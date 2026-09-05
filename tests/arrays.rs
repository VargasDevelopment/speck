use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ENTRIES: &str = "start {}\nupdate(dt: f32) {}\ndraw {}\n";

#[test]
fn arrays_lower_to_native_llvm_aggregates_with_checked_indexing() {
    let source = r#"game "Arrays"
const COUNT: i32 = 3
const FLAGS: [bool; COUNT] = [true, false, true]
let values: [i32; COUNT] = [10, 20, 30]
start {
    let local: [f32; 2] = [1.0, 2.0]
    let index: i32 = 1
    values[index] += 2
    print_i32(values[0])
    print_i32(values[index])
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("array program should compile");
    assert!(ir.contains("@spk_const_FLAGS = internal constant [3 x i1]"));
    assert!(ir.contains("@spk_global_values = internal global [3 x i32]"));
    assert!(ir.contains("alloca [2 x float]"));
    assert!(ir.contains("getelementptr inbounds [3 x i32]"));
    assert!(ir.contains("call void @crumb_bounds_fail(i32"));
}

#[test]
fn array_diagnostics_cover_shape_types_bounds_and_mutability() {
    assert_error(
        &format!("game \"Bad\"\nlet a: [i32; 3] = [1, 2]\n{ENTRIES}"),
        "expected array length 3, found 2 elements",
    );
    assert_error(
        &format!("game \"Bad\"\nlet a: [i32; 2] = [1, true]\n{ENTRIES}"),
        "array element expects `i32`, but found `bool`",
    );
    assert_error(
        &format!("game \"Bad\"\nlet a: [i32; 0] = []\n{ENTRIES}"),
        "array length must be positive, found 0",
    );
    assert_error(
        &format!("game \"Bad\"\nlet a: [i32; -2] = [1, 2]\n{ENTRIES}"),
        "array length must be positive, found -2",
    );
    assert_error(
        "game \"Bad\"\nlet a: [i32; 2] = [1, 2]\nstart { print_i32(a[2]) }\nupdate(dt: f32) {}\ndraw {}\n",
        "constant index 2 is out of bounds for length 2",
    );
    assert_error(
        "game \"Bad\"\nlet a: [i32; 2] = [1, 2]\nstart { print_i32(a[true]) }\nupdate(dt: f32) {}\ndraw {}\n",
        "array index must be i32",
    );
    assert_error(
        "game \"Bad\"\nconst A: [i32; 2] = [1, 2]\nstart { A[0] = 3 }\nupdate(dt: f32) {}\ndraw {}\n",
        "cannot assign through constant `A`",
    );
}

#[test]
fn constant_lengths_and_constant_indices_are_resolved() {
    let source = r#"game "Constants"
const TWO: i32 = 1 + 1
const FROM_ARRAY: i32 = SOURCE[0]
const DERIVED: [i32; FROM_ARRAY] = [3, 5]
const SOURCE: [i32; 1] = [TWO]
const VALUES: [i32; TWO] = [7, 9]
start { print_i32(VALUES[TWO - 1]) }
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("constant-sized array should compile");
    assert!(ir.contains("@spk_const_VALUES = internal constant [2 x i32]"));
    assert!(ir.contains("@spk_const_DERIVED = internal constant [2 x i32]"));

    assert_error(
        "game \"Bad\"\nconst TWO: i32 = 2\nconst VALUES: [i32; TWO] = [7, 9]\nstart { print_i32(VALUES[TWO]) }\nupdate(dt: f32) {}\ndraw {}\n",
        "constant index 2 is out of bounds for length 2",
    );
}

#[test]
fn aggregate_equality_is_rejected_before_llvm_lowering() {
    assert_error(
        "game \"Bad\"\nlet a: [i32; 2] = [1, 2]\nlet b: [i32; 2] = [1, 2]\nstart { let same: bool = a == b }\nupdate(dt: f32) {}\ndraw {}\n",
        "equality comparison requires scalar operands",
    );
}

#[test]
fn shadowed_constant_indices_follow_lexical_bindings() {
    let cases = [
        ("local", "", "let INDEX: i32 = 0 print_i32(values[INDEX])"),
        (
            "parameter",
            "fn read(INDEX: i32) -> i32 { return values[INDEX] }",
            "print_i32(read(0))",
        ),
        (
            "loop variable",
            "",
            "for INDEX in 0..2 { values[INDEX] += 1 }",
        ),
        (
            "nested local",
            "",
            "let INDEX: i32 = 0 if true { let INDEX: i32 = 1 values[INDEX] = 7 } print_i32(values[INDEX])",
        ),
        (
            "expression",
            "",
            "let INDEX: i32 = 0 print_i32(values[INDEX + 1])",
        ),
        (
            "array binding",
            "",
            "let INDICES: [i32; 1] = [0] print_i32(values[INDICES[0]])",
        ),
        (
            "struct binding",
            "",
            "let POSITION: Position = Position { index: 0 } print_i32(values[POSITION.index])",
        ),
    ];
    for (name, functions, body) in cases {
        let source = format!(
            "game \"Shadowed indices\"\n\
             struct Position {{ index: i32 }}\n\
             const INDEX: i32 = 9\n\
             const INDICES: [i32; 1] = [9]\n\
             const POSITION: Position = Position {{ index: 9 }}\n\
             let values: [i32; 2] = [10, 20]\n\
             {functions}\n\
             start {{ {body} }}\nupdate(dt: f32) {{}}\ndraw {{}}\n"
        );
        speck::analyze(&source)
            .unwrap_or_else(|errors| panic!("{name} shadowing should be accepted: {errors:#?}"));
    }
}

#[test]
fn constant_indices_are_checked_outside_shadowing_scopes() {
    for body in [
        "print_i32(values[INDEX])",
        "let INDEX: i32 = values[INDEX]",
        "if true { let INDEX: i32 = 0 } print_i32(values[INDEX])",
        "for INDEX in 0..1 {} values[INDEX] = 7",
        "print_i32(read(0)) values[INDEX] += 1",
        "print_i32(values[KEY_ESCAPE])",
    ] {
        let source = format!(
            "game \"Unshadowed indices\"\n\
             const INDEX: i32 = 10\n\
             let values: [i32; 2] = [10, 20]\n\
             fn read(INDEX: i32) -> i32 {{ return INDEX }}\n\
             start {{ {body} }}\nupdate(dt: f32) {{}}\ndraw {{}}\n"
        );
        assert_error(&source, "constant index 10 is out of bounds for length 2");
    }
}

#[test]
fn whole_array_copy_and_repeated_indexing_use_value_semantics() {
    let source = r#"game "Array Values"
let values: [i32; 2] = [3, 4]
start {
    let copy: [i32; 2] = values
    let matrix: [[i32; 2]; 2] = [[1, 2], [3, 4]]
    copy[0] = 9
    matrix[1][0] += 5
    print_i32(values[0])
    print_i32(copy[0])
    print_i32(matrix[1][0])
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("nested and copied arrays should compile");
    assert!(ir.contains("alloca [2 x [2 x i32]]"));
    assert!(ir.matches("getelementptr inbounds").count() >= 5);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/array_copy_e2e");
    fs::create_dir_all(&work).expect("array copy directory should exist");
    let source_path = work.join("array_copy.spk");
    fs::write(&source_path, source).expect("array copy source should be written");
    let executable = build_in(&work, &source_path);
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("array copy example should start");
    assert_success("array copy example", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3\n9\n8\n");
}

#[test]
fn arrays_are_deliberately_rejected_in_function_signatures() {
    assert_error(
        "game \"Bad\"\nfn first(values: [i32; 2]) -> i32 { return values[0] }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "arrays are not supported as function parameters yet",
    );
    assert_error(
        "game \"Bad\"\nfn values() -> [i32; 2] { return [1, 2] }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "arrays are not supported as function return types yet",
    );
}

#[test]
fn primitive_array_example_builds_verifies_and_executes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/array_values_e2e");
    fs::create_dir_all(&work).expect("array test directory should exist");
    let executable = build_in(&work, &root.join("examples/array_values.spk"));
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("array example should start");
    assert_success("array example", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "20\n11\n");

    let llvm = work.join("build/array_values.ll");
    let bitcode = work.join("verified.bc");
    let verify = Command::new("llvm-as")
        .arg(&llvm)
        .arg("-o")
        .arg(&bitcode)
        .output()
        .expect("llvm-as should start");
    assert_success("array LLVM verification", &verify);
}

#[test]
fn dynamic_and_negative_indices_fail_predictably_at_runtime() {
    for (name, index) in [("high", 3), ("negative", -1)] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let work = root.join(format!("target/array_bounds_{name}"));
        fs::create_dir_all(&work).expect("bounds test directory should exist");
        let source = work.join(format!("{name}.spk"));
        fs::write(
            &source,
            format!(
                "game \"Bounds\"\nlet index: i32 = {index}\nlet values: [i32; 3] = [1, 2, 3]\nstart {{ print_i32(values[index]) }}\nupdate(dt: f32) {{}}\ndraw {{}}\n"
            ),
        )
        .expect("bounds source should be written");
        let executable = build_in(&work, &source);
        let ir = fs::read_to_string(work.join("build").join(format!("{name}.ll")))
            .expect("bounds LLVM should remain inspectable");
        let failure = ir
            .find("\nbounds_failure")
            .expect("bounds failure block should exist");
        let call = ir
            .find("call void @crumb_bounds_fail")
            .expect("bounds failure hook should be called");
        let unreachable = ir[call..]
            .find("unreachable")
            .map(|offset| call + offset)
            .expect("bounds failure should terminate control flow");
        let valid = ir
            .find("\nbounds_valid")
            .expect("bounds valid block should exist");
        let element_address = ir
            .find("getelementptr inbounds [3 x i32]")
            .expect("element address should exist on the valid path");
        assert!(failure < call && call < unreachable && unreachable < valid);
        assert!(valid < element_address);

        let run = Command::new(executable)
            .current_dir(&work)
            .output()
            .expect("bounds example should start");
        assert!(!run.status.success(), "out-of-bounds access must fail");
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            format!("Speck array index {index} is out of bounds for length 3\n")
        );
        assert!(run.stdout.is_empty(), "failure must stop before the read");
    }
}

fn assert_error(source: &str, expected: &str) {
    let errors = speck::analyze(source).expect_err("source should be rejected");
    assert!(
        errors.iter().any(|error| error.message.contains(expected)),
        "expected diagnostic containing {expected:?}, found: {errors:#?}"
    );
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("array build", &build);
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
