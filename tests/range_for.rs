use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn basic_zero_and_nonzero_ranges_execute_exclusively() {
    let output = run_source(
        "range_shapes",
        r#"game "Ranges"
start {
    for i in 0..3 { print_i32(i) }
    for skipped in 4..4 { print_i32(skipped) }
    for skipped in 5..2 { print_i32(skipped) }
    for i in -2..1 { print_i32(i) }
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0\n1\n2\n-2\n-1\n0\n"
    );
}

#[test]
fn range_bounds_are_evaluated_once_before_iteration() {
    let output = run_source(
        "range_once",
        r#"game "Range Once"
let calls: i32 = 0
fn lower() -> i32 {
    calls += 1
    return 1
}
fn upper() -> i32 {
    calls += 1
    return 4
}
start {
    for i in lower()..upper() {
        print_i32(i)
    }
    print_i32(calls)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n3\n2\n");
}

#[test]
fn nested_loops_shadow_without_leaking_the_loop_variable() {
    let output = run_source(
        "range_shadowing",
        r#"game "Range Shadowing"
start {
    let i: i32 = 9
    for i in 0..2 {
        print_i32(i)
        for i in 3..5 { print_i32(i) }
        print_i32(i)
    }
    print_i32(i)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0\n3\n4\n0\n1\n3\n4\n1\n9\n"
    );

    assert_error(
        r#"game "Bad Scope"
start {
    for i in 0..1 {}
    print_i32(i)
}
update(dt: f32) {}
draw {}
"#,
        "unknown variable `i`",
    );
}

#[test]
fn loop_variables_are_read_only_and_bounds_are_i32() {
    assert_error(
        r#"game "Bad Mutation"
start {
    for i in 0..2 { i += 1 }
}
update(dt: f32) {}
draw {}
"#,
        "loop variable `i` is read-only",
    );
    assert_error(
        r#"game "Bad Lower"
start { for i in 0.0..2 {} }
update(dt: f32) {}
draw {}
"#,
        "range lower bound must be `i32`, found `f32`",
    );
    assert_error(
        r#"game "Bad Upper"
start { for i in 0..true {} }
update(dt: f32) {}
draw {}
"#,
        "range upper bound must be `i32`, found `bool`",
    );
}

#[test]
fn shadowing_struct_names_can_bound_a_range() {
    let output = run_source(
        "range_bound_shadowing",
        r#"game "Range Bound Shadowing"
struct Limit {}
fn print_to(Limit: i32) -> void {
    for i in 0..Limit { print_i32(i) }
}
start {
    let Limit: i32 = 2
    for i in 0..Limit { print_i32(i) }
    print_to(3)
    for Limit in 0..2 {
        for j in 0..Limit { print_i32(j) }
    }
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "0\n1\n0\n1\n2\n0\n"
    );
}

#[test]
fn final_platform_example_iterates_struct_array_and_verifies_llvm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/range_platform_e2e");
    fs::create_dir_all(&work).expect("range platform directory should exist");
    let executable = build_in(&work, &root.join("examples/platform_array.spk"));
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("range platform example should start");
    assert_success("range platform example", &run);
    assert!(run.stdout.is_empty());

    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame should be written");
    let pixel = |x: usize, y: usize| {
        const HEADER: usize = 15;
        let offset = HEADER + (y * 320 + x) * 3;
        &ppm[offset..offset + 3]
    };
    assert_eq!(pixel(40, 140), [255, 255, 255]);
    assert_eq!(pixel(150, 105), [255, 255, 255]);
    assert_eq!(pixel(240, 70), [255, 255, 255]);

    let ir = fs::read_to_string(work.join("build/platform_array.ll"))
        .expect("generated range LLVM should be inspectable");
    assert!(ir.contains("for_condition"));
    assert!(ir.contains("for_body"));
    assert!(ir.contains("icmp slt i32"));
    assert!(!ir.contains("while_condition"));

    let verify = Command::new("llvm-as")
        .arg(work.join("build/platform_array.ll"))
        .arg("-o")
        .arg(work.join("verified.bc"))
        .output()
        .expect("llvm-as should start");
    assert_success("range LLVM verification", &verify);
}

fn assert_error(source: &str, expected: &str) {
    let errors = speck::analyze(source).expect_err("source should be rejected");
    assert!(
        errors.iter().any(|error| error.message.contains(expected)),
        "expected diagnostic containing {expected:?}, found: {errors:#?}"
    );
}

fn run_source(name: &str, source: &str) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target").join(name);
    fs::create_dir_all(&work).expect("range test directory should exist");
    let source_path = work.join(format!("{name}.spk"));
    fs::write(&source_path, source).expect("range source should be written");
    let executable = build_in(&work, &source_path);
    let output = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("range executable should start");
    assert_success(name, &output);
    output
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("range build", &build);
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
