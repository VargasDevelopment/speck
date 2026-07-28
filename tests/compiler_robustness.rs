use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use speck::diagnostic::Diagnostic;

const CASES: usize = 5_000;
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
    let mut random = Random::new(SEED);

    for case in 0..CASES {
        let seed = PROGRAM_SEEDS[random.index(PROGRAM_SEEDS.len())];
        let source = mutate(seed, case, &mut random);
        assert!(source.len() <= MAX_SOURCE_BYTES);

        let outcome = catch_unwind(AssertUnwindSafe(|| exercise(&source)));
        if let Err(payload) = outcome {
            panic!(
                "compiler robustness case {case} panicked (seed {SEED:#x}): {}\n--- source ---\n{source}\n--- end source ---",
                panic_message(payload.as_ref())
            );
        }
    }
}

#[test]
fn bounded_deep_nesting_is_handled_without_panicking() {
    let nested = "(".repeat(64) + "1" + &")".repeat(64);
    let source = format!(
        "game \"Nested\"\nstart {{ let value: i32 = {nested} print_i32(value) }}\nupdate(dt: f32) {{}}\ndraw {{}}\n"
    );
    exercise(&source);
    speck::compile_to_llvm(&source).expect("bounded nested expressions should compile");
}

#[test]
fn robustness_seed_programs_are_valid() {
    for source in PROGRAM_SEEDS {
        speck::compile_to_llvm(source).unwrap_or_else(|diagnostics| {
            panic!(
                "robustness seed should compile:\n{}",
                speck::render_diagnostics(Path::new("seed.spk"), source, &diagnostics)
            )
        });
    }
}

#[test]
fn every_seed_truncation_reports_safely() {
    for source in PROGRAM_SEEDS {
        for boundary in char_boundaries(source) {
            exercise(&source[..boundary]);
        }
    }
}

fn exercise(source: &str) {
    match speck::compile_to_llvm(source) {
        Ok(ir) => {
            assert!(ir.contains("; Speck game:"));
            assert!(ir.contains("define void @spk_start()"));
        }
        Err(diagnostics) => {
            validate_diagnostics(source, &diagnostics);
            let rendered =
                speck::render_diagnostics(Path::new("robustness-input.spk"), source, &diagnostics);
            assert!(!rendered.is_empty());
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
