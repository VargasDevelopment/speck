use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn remainder_follows_truncated_division_sign_rules() {
    let output = run_source(
        "modulo_signs",
        r#"game "Modulo Signs"
start {
    print_i32(7 % 3)
    print_i32(-7 % 3)
    print_i32(7 % -3)
    print_i32(-7 % -3)
    print_i32(0 % 5)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n-1\n1\n-1\n0\n");
}

#[test]
fn remainder_binds_like_multiplicative_operators() {
    let output = run_source(
        "modulo_precedence",
        r#"game "Modulo Precedence"
start {
    print_i32(10 + 7 % 3 * 2)
    print_i32((10 + 7) % (3 * 2))
    print_i32(100 / 7 % 5)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "12\n5\n4\n");
}

#[test]
fn compound_remainder_mutates_locals_globals_and_paths() {
    let output = run_source(
        "modulo_compound",
        r#"game "Modulo Compound"
struct Slot {
    capacity: i32
}
let total: i32 = 17
let slots: [Slot; 2] = [Slot { capacity: 29 }, Slot { capacity: 44 }]
fn shrink(value: i32) -> i32 {
    value %= 5
    return value
}
start {
    total %= 10
    print_i32(total)
    let local: i32 = 23
    local %= 4
    print_i32(local)
    slots[0].capacity %= 7
    print_i32(slots[0].capacity)
    print_i32(shrink(38))
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n3\n1\n3\n");
}

#[test]
fn constant_remainder_evaluates_and_rejects_invalid_constants() {
    let output = run_source(
        "modulo_const",
        r#"game "Modulo Const"
const WIDTH: i32 = 320
const CELLS: i32 = WIDTH % 60
start { print_i32(CELLS) }
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "20\n");

    assert_error(
        r#"game "Modulo Const Zero"
const BROKEN: i32 = 10 % 0
start {}
update(dt: f32) {}
draw {}
"#,
        "remainder by zero in constant expression",
    );
    assert_error(
        r#"game "Modulo Const Overflow"
const MINIMUM: i32 = -2147483648
const BROKEN: i32 = MINIMUM % -1
start {}
update(dt: f32) {}
draw {}
"#,
        "constant-expression overflow",
    );
}

#[test]
fn float_and_boolean_remainder_are_rejected() {
    assert_error(
        r#"game "Float Modulo"
let left: f32 = 7.5
let right: f32 = 2.0
start { let result: f32 = left % right }
update(dt: f32) {}
draw {}
"#,
        "remainder requires `i32` operands, found `f32`",
    );
    assert_error(
        r#"game "Float Compound Remainder"
let value: f32 = 7.5
start { value %= 2.0 }
update(dt: f32) {}
draw {}
"#,
        "remainder requires `i32` operands, but found `f32`",
    );
    assert_error(
        r#"game "Bool Modulo"
start { let result: i32 = true % false }
update(dt: f32) {}
draw {}
"#,
        "remainder requires `i32` operands, found `bool`",
    );
}

#[test]
fn immutable_roots_reject_remainder_assignment() {
    assert_error(
        r#"game "Const Remainder"
const C: i32 = 5
start { C %= 2 }
update(dt: f32) {}
draw {}
"#,
        "cannot use compound assignment on constant `C`",
    );
}

#[test]
fn invalid_runtime_remainder_terminates_before_srem() {
    let zero_source = r#"game "Remainder Zero"
fn dividend() -> i32 { print_i32(1) return 10 }
fn divisor() -> i32 { print_i32(2) return 0 }
start { print_i32(dividend() % divisor()) }
update(dt: f32) {}
draw {}
"#;
    let overflow_source = r#"game "Remainder Overflow"
let dividend: i32 = i32(-2147483648.0)
let divisor: i32 = -1
start { print_i32(dividend % divisor) }
update(dt: f32) {}
draw {}
"#;

    let ir = speck::compile_to_llvm(zero_source).expect("checked remainder should compile");
    let failure = ir
        .find("\nremainder_failure")
        .expect("remainder failure block should exist");
    let call = ir
        .find("call void @crumb_remainder_fail")
        .expect("remainder failure hook should be called");
    let valid = ir
        .find("\nremainder_valid")
        .expect("remainder valid block should exist");
    let srem = ir.find(" = srem i32").expect("valid srem should exist");
    assert!(failure < call && call < valid && valid < srem);

    let zero = run_failing_source("modulo_zero", zero_source);
    assert_eq!(
        String::from_utf8_lossy(&zero.stderr),
        "Speck integer remainder by zero: 10 % 0\n"
    );
    assert_eq!(String::from_utf8_lossy(&zero.stdout), "1\n2\n");

    let overflow = run_failing_source("modulo_overflow", overflow_source);
    assert_eq!(
        String::from_utf8_lossy(&overflow.stderr),
        "Speck integer remainder overflow: -2147483648 % -1\n"
    );
    assert!(overflow.stdout.is_empty());
}

#[test]
fn short_circuit_skips_remainder_side_effects() {
    let output = run_source(
        "modulo_short_circuit",
        r#"game "Modulo Short Circuit"
let zero: i32 = 0
start {
    if false && 10 % zero == 0 { print_i32(99) }
    if true || 10 % zero == 0 { print_i32(1) }
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n");
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
    fs::create_dir_all(&work).expect("modulo test directory should exist");
    let source_path = work.join(format!("{name}.spk"));
    fs::write(&source_path, source).expect("modulo source should be written");
    let executable = build_in(&work, &source_path);
    let output = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("modulo executable should start");
    assert_success(name, &output);
    output
}

fn run_failing_source(name: &str, source: &str) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target").join(name);
    fs::create_dir_all(&work).expect("modulo test directory should exist");
    let source_path = work.join(format!("{name}.spk"));
    fs::write(&source_path, source).expect("modulo source should be written");
    let executable = build_in(&work, &source_path);
    Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("modulo executable should start")
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("modulo build", &build);
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
