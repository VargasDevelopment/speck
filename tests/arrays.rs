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
const VALUES: [i32; TWO] = [7, 9]
start { print_i32(VALUES[TWO - 1]) }
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("constant-sized array should compile");
    assert!(ir.contains("@spk_const_VALUES = internal constant [2 x i32]"));

    assert_error(
        "game \"Bad\"\nconst TWO: i32 = 2\nconst VALUES: [i32; TWO] = [7, 9]\nstart { print_i32(VALUES[TWO]) }\nupdate(dt: f32) {}\ndraw {}\n",
        "constant index 2 is out of bounds for length 2",
    );
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
        let run = Command::new(executable)
            .current_dir(&work)
            .output()
            .expect("bounds example should start");
        assert!(!run.status.success(), "out-of-bounds access must fail");
        assert_eq!(
            String::from_utf8_lossy(&run.stderr),
            format!("Speck array index {index} is out of bounds for length 3\n")
        );
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
