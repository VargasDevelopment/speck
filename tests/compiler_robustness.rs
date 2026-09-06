pub mod support;

use std::any::Any;
use std::collections::BTreeSet;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use speck::diagnostic::Diagnostic;

const CASES: usize = 5_000;
const VERIFIED_MUTATIONS: usize = 16;
const MAX_SOURCE_BYTES: usize = 2_048;
const SEED: u64 = 0x5EEC_C0DE_D15C_A11E;

const PROGRAM_SEEDS: &[&str] = &[
    r#"game "Minimal"
start {}
update(dt: f32) {}
draw {}
"#,
    r#"game "Control Flow"
const LIMIT: i32 = 3
fn choose(value: i32) -> i32 {
    if value > 1 { return value }
    else { return 1 }
}
start {
    let total: i32 = 0
    for i in 0..LIMIT { total += choose(i) }
    while total < 8 { total += 1 }
    print_i32(total)
}
update(dt: f32) {}
draw {}
"#,
    r#"game "Aggregates"
struct Point {
    x: i32
    flags: [bool; 2]
}
const COUNT: i32 = 2
const POINTS: [Point; COUNT] = [
    Point { x: 4, flags: [true, false] },
    Point { flags: [false, true], x: 9 }
]
fn copy(point: Point) -> Point { return point }
start {
    let points: [Point; COUNT] = POINTS
    points[0].x += 1
    let point: Point = copy(points[1])
    if point.flags[1] { print_i32(point.x) }
}
update(dt: f32) {}
draw {}
"#,
    r#"game "Expressions"
const ENABLED: bool = true && !false
const HALF: f32 = f32(3) / 2.0
fn calculate(left: i32, right: i32) -> i32 {
    return (left + right) * (left - right)
}
start {
    let answer: i32 = calculate(7, 2)
    if ENABLED && answer != 0 { print_i32(i32(HALF) + answer) }
}
update(dt: f32) {}
draw {}
"#,
    r#"game "Unicode title 🦀"
// Non-ASCII is valid inside strings and invalid elsewhere with a diagnostic.
start { print_i32(1) }
update(dt: f32) {}
draw {}
"#,
];

// Token replacements preserve well-formed programs while changing operands,
// operations, and control flow. Arbitrary byte edits mostly yield diagnostics
// or title changes, so they cannot supply meaningful accepted-case coverage alone.
const SEMANTIC_MUTATIONS: &[(usize, &str, &str)] = &[
    (1, "LIMIT: i32 = 3", "LIMIT: i32 = 0"),
    (1, "LIMIT: i32 = 3", "LIMIT: i32 = 4"),
    (1, "value > 1", "value >= 1"),
    (1, "return 1", "return -1"),
    (1, "total += choose(i)", "total *= choose(i)"),
    (1, "while total < 8", "while total > 8"),
    (2, "x: 4", "x: -4"),
    (2, "[true, false]", "[false, true]"),
    (2, "points[0].x += 1", "points[0].x %= 3"),
    (2, "return point", "point.x += 2 return point"),
    (2, "copy(points[1])", "copy(points[0])"),
    (3, "true && !false", "false || !true"),
    (3, "f32(3) / 2.0", "f32(3) * 2.0"),
    (
        3,
        "(left + right) * (left - right)",
        "(left + right) / (left - right)",
    ),
    (3, "calculate(7, 2)", "calculate(2, 7)"),
    (3, "answer != 0", "answer <= 0"),
];

const FRAGMENTS: &[&str] = &[
    "",
    " ",
    "\n",
    "game",
    "struct",
    "const",
    "let",
    "fn",
    "start",
    "update",
    "draw",
    "return",
    "if",
    "else",
    "while",
    "for",
    "in",
    "i32",
    "f32",
    "bool",
    "void",
    "true",
    "false",
    "identifier",
    "0",
    "2147483648",
    "1.0",
    "\"text\"",
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    ".",
    "..",
    ":",
    ",",
    ";",
    "+",
    "-",
    "*",
    "/",
    "=",
    "==",
    "!=",
    "+=",
    "&&",
    "||",
    "->",
    "// comment\n",
    "é",
    "🦀",
];

#[test]
fn deterministic_source_mutations_never_panic_or_produce_invalid_spans() {
    let Some(directory) =
        isolated(
            "deterministic_source_mutations_never_panic_or_produce_invalid_spans",
            |work| {
                let mut random = Random::new(SEED);
                let mut seed_ir = BTreeSet::new();
                for (index, source) in PROGRAM_SEEDS.iter().enumerate() {
                    record_case(work, &format!("program seed {index}"), source);
                    seed_ir.insert(ir_fingerprint(
                        &exercise(source).expect("robustness seed should compile"),
                    ));
                }
                let mut verified_ir = BTreeSet::new();

                let random_mutations = (0..CASES).map(|case| {
                    let seed = PROGRAM_SEEDS[random.index(PROGRAM_SEEDS.len())];
                    (
                        format!("mutation {case}, seed {SEED:#x}"),
                        mutate(seed, case, &mut random),
                        false,
                    )
                });
                let semantic_mutations = SEMANTIC_MUTATIONS.iter().enumerate().map(
                    |(case, &(index, from, to))| {
                        let seed = PROGRAM_SEEDS[index];
                        assert_eq!(seed.matches(from).count(), 1, "unique replacement target");
                        (
                            format!(
                                "semantic mutation {case}, program seed {index}: {from:?} -> {to:?}"
                            ),
                            seed.replacen(from, to, 1),
                            true,
                        )
                    },
                );
                for (name, source, must_compile) in random_mutations.chain(semantic_mutations) {
                    assert!(source.len() <= MAX_SOURCE_BYTES);
                    record_case(work, &name, &source);
                    let ir = exercise(&source);
                    assert!(!must_compile || ir.is_some(), "{name} should compile");
                    if let Some(ir) = ir
                        && verified_ir.len() < VERIFIED_MUTATIONS
                        && !PROGRAM_SEEDS.contains(&source.as_str())
                    {
                        let fingerprint = ir_fingerprint(&ir);
                        if seed_ir.contains(&fingerprint) || !verified_ir.insert(fingerprint) {
                            continue;
                        }
                        // First-seen order is deterministic. The shared fingerprint
                        // excludes seed-equivalent and duplicate modules even when
                        // their title comments differ. Never execute these mutations.
                        let sample = verified_ir.len() - 1;
                        fs::write(work.join(format!("mutation-{sample}.ll")), ir)
                            .expect("mutation IR should write");
                        fs::copy(
                            work.join("active-case.txt"),
                            work.join(format!("mutation-{sample}.txt")),
                        )
                        .expect("selected mutation context should persist");
                    }
                }
                assert_eq!(
                    verified_ir.len(),
                    VERIFIED_MUTATIONS,
                    "the corpus must supply a full sample of distinct accepted mutations"
                );
            },
        )
    else {
        return;
    };
    // The compiler child launches no external tools. Only this surviving
    // parent supervises verifier process groups, so killing a stuck compiler
    // cannot leave an independently grouped Clang process without its owner.
    let work = directory.path();
    for sample in 0..VERIFIED_MUTATIONS {
        let name = format!("LLVM verification for sample {sample}");
        with_case_context(&work.join(format!("mutation-{sample}.txt")), &name, || {
            support::assert_success(
                &name,
                &support::verify_ir(
                    &work.join(format!("mutation-{sample}.ll")),
                    &work.join(format!("mutation-{sample}.bc")),
                ),
            );
        });
    }
}

#[test]
fn bounded_deep_nesting_is_handled_without_panicking() {
    let _ = isolated(
        "bounded_deep_nesting_is_handled_without_panicking",
        |work| {
            let nested = "(".repeat(64) + "1" + &")".repeat(64);
            let source = format!(
                "game \"Nested\"\nstart {{ let value: i32 = {nested} print_i32(value) }}\nupdate(dt: f32) {{}}\ndraw {{}}\n"
            );
            record_case(work, "64 nested parentheses", &source);
            exercise(&source).expect("bounded nested expressions should compile");
        },
    );
}

#[test]
fn robustness_seed_programs_are_valid() {
    let _ = isolated("robustness_seed_programs_are_valid", |work| {
        for (index, source) in PROGRAM_SEEDS.iter().enumerate() {
            record_case(work, &format!("program seed {index}"), source);
            let ir = exercise(source).expect("robustness seed should compile");
            // Regression: a changed game-title comment is not a new module.
            let retitled = source.replacen("game \"", "game \"retitled ", 1);
            record_case(work, &format!("retitled program seed {index}"), &retitled);
            let retitled_ir = exercise(&retitled).expect("retitled seed should compile");
            assert_ne!(
                ir, retitled_ir,
                "the regression must change the emitted title"
            );
            assert_eq!(ir_fingerprint(&ir), ir_fingerprint(&retitled_ir));
        }
    });
}

#[test]
fn every_seed_truncation_reports_safely() {
    let _ = isolated("every_seed_truncation_reports_safely", |work| {
        for (index, source) in PROGRAM_SEEDS.iter().enumerate() {
            for boundary in char_boundaries(source) {
                let source = &source[..boundary];
                record_case(
                    work,
                    &format!("seed {index}, truncation {boundary}"),
                    source,
                );
                exercise(source);
            }
        }
    });
}

// Ignore comments and insignificant line whitespace, preserving quoted IR
// strings (including semicolons) and all instructions, operands, and globals.
fn ir_fingerprint(ir: &str) -> String {
    ir.lines()
        .map(|line| {
            let mut quoted = false;
            let comment = line.char_indices().find_map(|(index, character)| {
                if character == '"' {
                    quoted = !quoted;
                }
                (character == ';' && !quoted).then_some(index)
            });
            line[..comment.unwrap_or(line.len())].trim()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// The child still uses an ordinary test thread stack. Panics, stack-overflow
// aborts, and compiler hangs become failures of the parent instead of killing
// or hanging the suite. Persist the active input before entering the compiler,
// so even an abort/timeout reports a reproducible case rather than losing it.
// Only the parent receives the workspace, allowing post-compilation tools to
// run under its supervision after the bounded compiler child has exited.
fn isolated(name: &str, test: impl FnOnce(&Path)) -> Option<tempfile::TempDir> {
    const CHILD: &str = "SPECK_ROBUSTNESS_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        test(&std::env::current_dir().expect("child workspace"));
        return None;
    }
    let directory = support::workspace();
    let work = directory.path();
    with_case_context(&work.join("active-case.txt"), name, || {
        let output = support::run_with_timeout(
            Command::new(std::env::current_exe().expect("test executable"))
                .current_dir(work)
                .args(["--exact", name, "--nocapture"])
                .env(CHILD, name),
            Duration::from_secs(60),
        );
        support::assert_success(name, &output);
    });
    Some(directory)
}

fn with_case_context(path: &Path, name: &str, test: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(test)) {
        let active = fs::read_to_string(path).unwrap_or_default();
        panic!(
            "{name}: {}\n--- active case and source ---\n{active}\n--- end source ---",
            panic_message(payload.as_ref())
        );
    }
}

fn record_case(work: &Path, name: &str, source: &str) {
    fs::write(work.join("active-case.txt"), format!("{name}\n{source}"))
        .expect("active robustness input should write");
}

fn exercise(source: &str) -> Option<String> {
    match speck::compile_to_llvm(source) {
        Ok(ir) => {
            assert!(ir.contains("; Speck game:"));
            assert!(ir.contains("define void @spk_start()"));
            Some(ir)
        }
        Err(diagnostics) => {
            validate_diagnostics(source, &diagnostics);
            let rendered =
                speck::render_diagnostics(Path::new("robustness-input.spk"), source, &diagnostics);
            assert!(!rendered.is_empty());
            None
        }
    }
}

fn validate_diagnostics(source: &str, diagnostics: &[Diagnostic]) {
    assert!(!diagnostics.is_empty());
    for diagnostic in diagnostics {
        assert!(!diagnostic.message.is_empty());
        assert!(
            diagnostic.span.start <= diagnostic.span.end,
            "diagnostic span is reversed: {:?}",
            diagnostic.span
        );
        assert!(
            diagnostic.span.end <= source.len(),
            "diagnostic span {:?} exceeds source length {}",
            diagnostic.span,
            source.len()
        );
        assert!(source.is_char_boundary(diagnostic.span.start));
        assert!(source.is_char_boundary(diagnostic.span.end));
    }
}

fn mutate(seed: &str, case: usize, random: &mut Random) -> String {
    if case.is_multiple_of(37) {
        return seed.to_owned();
    }

    let mut source = seed.to_owned();
    let operations = 1 + random.index(4);
    for _ in 0..operations {
        match random.index(6) {
            0 => insert_fragment(&mut source, random),
            1 => delete_slice(&mut source, random),
            2 => replace_slice(&mut source, random),
            3 => duplicate_slice(&mut source, random),
            4 => truncate(&mut source, random),
            5 => source = token_soup(random),
            _ => unreachable!(),
        }
        shrink_to_limit(&mut source);
    }
    source
}

fn insert_fragment(source: &mut String, random: &mut Random) {
    let boundaries = char_boundaries(source);
    let at = boundaries[random.index(boundaries.len())];
    source.insert_str(at, FRAGMENTS[random.index(FRAGMENTS.len())]);
}

fn delete_slice(source: &mut String, random: &mut Random) {
    let (start, end) = random_slice(source, random);
    source.replace_range(start..end, "");
}

fn replace_slice(source: &mut String, random: &mut Random) {
    let (start, end) = random_slice(source, random);
    source.replace_range(start..end, FRAGMENTS[random.index(FRAGMENTS.len())]);
}

fn duplicate_slice(source: &mut String, random: &mut Random) {
    let (start, end) = random_slice(source, random);
    let duplicate = source[start..end].to_owned();
    let boundaries = char_boundaries(source);
    let at = boundaries[random.index(boundaries.len())];
    source.insert_str(at, &duplicate);
}

fn truncate(source: &mut String, random: &mut Random) {
    let boundaries = char_boundaries(source);
    source.truncate(boundaries[random.index(boundaries.len())]);
}

fn token_soup(random: &mut Random) -> String {
    let mut source = String::new();
    let fragments = 1 + random.index(48);
    for _ in 0..fragments {
        source.push_str(FRAGMENTS[random.index(FRAGMENTS.len())]);
        if random.index(3) == 0 {
            source.push(' ');
        }
    }
    source
}

fn random_slice(source: &str, random: &mut Random) -> (usize, usize) {
    let boundaries = char_boundaries(source);
    let left = random.index(boundaries.len());
    let right = random.index(boundaries.len());
    if left <= right {
        (boundaries[left], boundaries[right])
    } else {
        (boundaries[right], boundaries[left])
    }
}

fn char_boundaries(source: &str) -> Vec<usize> {
    source
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(source.len()))
        .collect()
}

fn shrink_to_limit(source: &mut String) {
    if source.len() <= MAX_SOURCE_BYTES {
        return;
    }
    let boundary = source.floor_char_boundary(MAX_SOURCE_BYTES);
    source.truncate(boundary);
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(message) = payload.downcast_ref::<&str>() {
        message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message
    } else {
        "non-string panic payload"
    }
}

struct Random(u64);

impl Random {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn index(&mut self, length: usize) -> usize {
        (self.next() as usize) % length
    }
}
