use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn structs_lower_to_named_fixed_layout_values() {
    let source = r#"game "Struct IR"
struct Point { x: i32, y: i32 }
const ORIGIN: Point = Point { y: 0, x: 0 }
let point: Point = ORIGIN
fn moved(value: Point) -> Point {
    value.x += 3
    return value
}
start {
    let local: Point = Point { x: point.x, y: point.y }
    point = local
    point = moved(point)
    print_i32(moved(point).x)
    print_i32(point.x)
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("struct program should compile");
    assert!(ir.contains("%spk_struct_Point = type { i32, i32 }"));
    assert!(ir.contains("@spk_const_ORIGIN = internal constant %spk_struct_Point"));
    assert!(ir.contains("define %spk_struct_Point @spk_fn_moved(%spk_struct_Point %arg0)"));
    assert!(ir.contains("getelementptr inbounds %spk_struct_Point"));
    assert!(ir.contains("insertvalue %spk_struct_Point"));
    assert!(ir.contains("extractvalue %spk_struct_Point"));
}

#[test]
fn struct_diagnostics_cover_declarations_literals_and_fields() {
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nstruct Point { y: i32 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "struct `Point` is declared more than once",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32, x: i32 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "field `x` is declared more than once",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32, y: i32 }\nlet p: Point = Point { x: 1 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "missing initializer for field `y`",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nlet p: Point = Point { x: 1, x: 2 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "field `x` is initialized more than once",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nlet p: Point = Point { z: 1 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "type `Point` has no field named `z`",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nlet p: Point = Point { x: true }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "field `x` of `Point` expects `i32`, but found `bool`",
    );
    assert_error(
        "game \"Bad\"\nlet p: Missing = Missing { x: 1 }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "unknown struct type `Missing`",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nlet p: Point = Point { x: 1 }\nstart { print_i32(p.xx) }\nupdate(dt: f32) {}\ndraw {}\n",
        "type `Point` has no field named `xx`",
    );
    assert_error(
        "game \"Bad\"\nstart { let x: i32 = 1 print_i32(x.value) }\nupdate(dt: f32) {}\ndraw {}\n",
        "type `i32` has no fields",
    );
}

#[test]
fn recursive_and_not_yet_composed_aggregate_types_are_rejected() {
    assert_error(
        "game \"Bad\"\nstruct Node { next: Node }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "recursive value type is not supported",
    );
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nlet points: [Point; 2] = [Point { x: 1 }, Point { x: 2 }]\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "arrays of structs are added by the aggregate-composition slice",
    );
    assert_error(
        "game \"Bad\"\nstruct State { values: [i32; 2] }\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        "aggregate-valued struct fields are added by the aggregate-composition slice",
    );
}

#[test]
fn constant_struct_fields_cannot_be_mutated() {
    assert_error(
        "game \"Bad\"\nstruct Point { x: i32 }\nconst ORIGIN: Point = Point { x: 0 }\nstart { ORIGIN.x += 1 }\nupdate(dt: f32) {}\ndraw {}\n",
        "cannot use compound assignment through constant `ORIGIN`",
    );
}

#[test]
fn aggregate_equality_and_shadowed_struct_names_are_handled_before_codegen() {
    assert_error(
        r#"game "Bad Equality"
struct Point { x: i32 }
let a: Point = Point { x: 1 }
let b: Point = Point { x: 1 }
start { let same: bool = a == b }
update(dt: f32) {}
draw {}
"#,
        "equality comparison requires scalar operands",
    );

    let source = r#"game "Shadowing"
struct Flag {}
fn check(Flag: bool) -> void {
    if Flag {}
    while Flag { return }
}
start {
    let Flag: bool = true
    if Flag { check(Flag) }
}
update(dt: f32) {}
draw {}
"#;
    speck::compile_to_llvm(source).expect("value bindings should shadow struct literal names");
}

#[test]
fn platform_value_example_builds_verifies_and_executes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/platform_value_e2e");
    fs::create_dir_all(&work).expect("struct test directory should exist");
    let executable = build_in(&work, &root.join("examples/platform_value.spk"));
    let run = Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("struct example should start");
    assert_success("struct example", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "40\n45\n");

    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame should be written");
    let pixel = |x: usize, y: usize| {
        const HEADER: usize = 15;
        let offset = HEADER + (y * 320 + x) * 3;
        &ppm[offset..offset + 3]
    };
    assert_eq!(pixel(45, 140), [255, 255, 255]);
    assert_eq!(pixel(44, 140), [32, 18, 32]);

    let verify = Command::new("llvm-as")
        .arg(work.join("build/platform_value.ll"))
        .arg("-o")
        .arg(work.join("verified.bc"))
        .output()
        .expect("llvm-as should start");
    assert_success("struct LLVM verification", &verify);
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
    assert_success("struct build", &build);
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
