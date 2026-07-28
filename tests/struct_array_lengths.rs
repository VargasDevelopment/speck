use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ENTRIES: &str = "start {}\nupdate(dt: f32) {}\ndraw {}\n";

#[test]
fn literal_array_length_resolves_in_struct_field() {
    let source =
        format!("game \"Literal struct array\"\nstruct Holder {{ values: [i32; 3] }}\n{ENTRIES}");
    let ir = speck::compile_to_llvm(&source).expect("literal struct array should compile");
    assert!(ir.contains("%spk_struct_Holder = type { [3 x i32] }"));
}

#[test]
fn constant_array_length_resolves_in_struct_field() {
    let source = format!(
        "game \"Constant struct array\"\nconst COUNT: i32 = 3\nstruct Holder {{ values: [i32; COUNT] }}\n{ENTRIES}"
    );
    let ir = speck::compile_to_llvm(&source).expect("constant struct array should compile");
    assert!(ir.contains("%spk_struct_Holder = type { [3 x i32] }"));
}

#[test]
fn constant_length_resolves_for_arrays_of_structs() {
    let source = format!(
        "game \"Struct element array\"\nconst COUNT: i32 = 3\nstruct Item {{ value: i32 }}\nstruct Holder {{ items: [Item; COUNT] }}\n{ENTRIES}"
    );
    let ir = speck::compile_to_llvm(&source).expect("array-of-struct field should compile");
    assert!(ir.contains("%spk_struct_Holder = type { [3 x %spk_struct_Item] }"));
}

#[test]
fn struct_array_elements_follow_existing_forward_reference_semantics() {
    for source in [
        format!(
            "game \"Forward struct\"\nconst COUNT: i32 = 2\nstruct Holder {{ items: [Item; COUNT] }}\nstruct Item {{ value: i32 }}\n{ENTRIES}"
        ),
        format!(
            "game \"Prior struct\"\nconst COUNT: i32 = 2\nstruct Item {{ value: i32 }}\nstruct Holder {{ items: [Item; COUNT] }}\n{ENTRIES}"
        ),
    ] {
        let ir = speck::compile_to_llvm(&source)
            .expect("struct array element order should follow forward-reference semantics");
        assert!(ir.contains("%spk_struct_Holder = type { [2 x %spk_struct_Item] }"));
    }
}

#[test]
fn constants_are_reusable_across_struct_global_and_local_array_types() {
    let source = r#"game "Reusable lengths"
const COUNT: i32 = 2
struct Item { value: i32 }
struct Holder { items: [Item; COUNT], values: [i32; COUNT] }
let global_values: [i32; COUNT] = [1, 2]
start { let local_values: [bool; COUNT] = [true, false] }
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("reused array length should compile");
    assert!(ir.contains("%spk_struct_Holder = type { [2 x %spk_struct_Item], [2 x i32] }"));
    assert!(ir.contains("@spk_global_global_values = internal global [2 x i32]"));
    assert!(ir.contains("alloca [2 x i1]"));
}

#[test]
fn different_constants_and_derived_expressions_resolve_independently() {
    let source = format!(
        "game \"Independent lengths\"\nconst WIDTH: i32 = 4\nconst COUNT: i32 = WIDTH * 2\nconst FLAGS: i32 = 3\nstruct Holder {{ values: [i32; COUNT], flags: [bool; FLAGS] }}\n{ENTRIES}"
    );
    let ir = speck::compile_to_llvm(&source).expect("derived array lengths should compile");
    assert!(ir.contains("%spk_struct_Holder = type { [8 x i32], [3 x i1] }"));
}

#[test]
fn direct_and_indirect_array_length_cycles_are_rejected_once() {
    for (source, expected_cycle) in [
        (
            format!(
                "game \"Direct cycle\"\nconst A: i32 = A\nstruct Bad {{ one: [i32; A], two: [i32; A] }}\n{ENTRIES}"
            ),
            "A -> A",
        ),
        (
            format!(
                "game \"Indirect cycle\"\nconst A: i32 = B\nconst B: i32 = A\nstruct Bad {{ values: [i32; A] }}\n{ENTRIES}"
            ),
            "A -> B -> A",
        ),
    ] {
        let errors = diagnostics(&source);
        let cycles = errors
            .iter()
            .filter(|error| error.message.contains("cyclic array-length constant"))
            .collect::<Vec<_>>();
        assert_eq!(
            cycles.len(),
            1,
            "expected one cycle diagnostic: {errors:#?}"
        );
        assert!(cycles[0].message.contains(expected_cycle));
        assert_eq!(errors.len(), 1, "cycle recovery emitted a derivative error");
        assert_unique(&errors);
    }
}

#[test]
fn ordinary_direct_and_indirect_constant_cycles_remain_rejected_once() {
    for source in [
        format!("game \"Direct cycle\"\nconst A: i32 = A\n{ENTRIES}"),
        format!("game \"Indirect cycle\"\nconst A: i32 = B\nconst B: i32 = A\n{ENTRIES}"),
    ] {
        let errors = diagnostics(&source);
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.message.contains("cyclic constant definition"))
                .count(),
            1,
            "expected one cycle diagnostic: {errors:#?}"
        );
        assert_eq!(errors.len(), 1, "cycle recovery emitted a derivative error");
        assert_unique(&errors);
    }
}

#[test]
fn failed_length_evaluation_does_not_leave_unrelated_constants_active() {
    let source = format!(
        "game \"Clean evaluator state\"\nconst BAD: i32 = MISSING\nconst GOOD: i32 = 2\nstruct Holder {{ before: [i32; GOOD], broken: [i32; BAD], after: [bool; GOOD] }}\n{ENTRIES}"
    );
    let errors = diagnostics(&source);
    assert!(errors.iter().any(|error| {
        error
            .message
            .contains("unknown array-length constant `MISSING`")
    }));
    assert!(
        errors.iter().all(|error| !error.message.contains("cyclic")),
        "a failed dependency must not leave GOOD marked active: {errors:#?}"
    );
    assert_eq!(
        errors.len(),
        1,
        "failed resolution emitted a derivative error"
    );
    assert_unique(&errors);
}

#[test]
fn invalid_lengths_do_not_create_placeholder_shape_or_duplicate_diagnostics() {
    let cases = [
        (
            format!(
                "game \"Wrong type\"\nconst COUNT: bool = true\nstruct Holder {{ values: [i32; COUNT] }}\nlet holder: Holder = Holder {{ values: [1, 2, 3] }}\n{ENTRIES}"
            ),
            "must have type `i32`",
        ),
        (
            format!(
                "game \"Zero\"\nstruct Holder {{ values: [i32; 0] }}\nlet holder: Holder = Holder {{ values: [1, 2, 3] }}\n{ENTRIES}"
            ),
            "array length must be positive, found 0",
        ),
        (
            format!(
                "game \"Negative\"\nstruct Holder {{ values: [i32; -2] }}\nlet holder: Holder = Holder {{ values: [1, 2, 3] }}\n{ENTRIES}"
            ),
            "array length must be positive, found -2",
        ),
        (
            format!(
                "game \"Unknown\"\nstruct Holder {{ values: [i32; MISSING] }}\nlet holder: Holder = Holder {{ values: [1, 2, 3] }}\n{ENTRIES}"
            ),
            "unknown array-length constant `MISSING`",
        ),
    ];

    for (source, expected) in cases {
        let errors = diagnostics(&source);
        assert!(
            errors.iter().any(|error| error.message.contains(expected)),
            "expected {expected:?}, found {errors:#?}"
        );
        assert!(
            errors
                .iter()
                .all(|error| !error.message.contains("expected array length 1")),
            "invalid lengths must not recover as length one: {errors:#?}"
        );
        assert_eq!(
            errors.len(),
            1,
            "invalid length emitted a derivative diagnostic: {errors:#?}"
        );
        assert_unique(&errors);
    }
}

#[test]
fn struct_constant_array_builds_verifies_and_runs_natively() {
    let source = r#"game "Struct Array Constant Native"
const MAX_PLATFORM_COUNT: i32 = 3
struct Platform { value: i32 }
struct Level { level_platforms: [Platform; MAX_PLATFORM_COUNT] }
let level_platforms: [Platform; MAX_PLATFORM_COUNT] = [
    Platform { value: 1 },
    Platform { value: 2 },
    Platform { value: 3 }
]
start { print_i32(level_platforms[2].value) }
update(dt: f32) {}
draw {}
"#;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/struct_array_constant_native");
    fs::create_dir_all(&work).expect("native regression directory should exist");
    let source_path = work.join("struct_array_constant.spk");
    fs::write(&source_path, source).expect("native regression source should be written");

    let executable = build_in(&work, &source_path);
    assert!(work.join("build/struct_array_constant.ll").is_file());
    assert!(
        work.join("build/struct_array_constant.bc").is_file(),
        "the build command should verify the generated LLVM and retain bitcode"
    );

    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("native regression executable should start");
    assert_success("native regression executable", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n");
}

fn diagnostics(source: &str) -> Vec<speck::diagnostic::Diagnostic> {
    speck::analyze(source).expect_err("source should be rejected")
}

fn assert_unique(errors: &[speck::diagnostic::Diagnostic]) {
    let unique = errors
        .iter()
        .map(|error| (error.message.as_str(), error.span))
        .collect::<HashSet<_>>();
    assert_eq!(
        unique.len(),
        errors.len(),
        "duplicate diagnostics: {errors:#?}"
    );
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("native regression build", &build);
    let stem = source.file_stem().expect("source should have a file stem");
    work.join("build").join(stem)
}

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
