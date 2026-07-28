use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[test]
fn accepted_feature_interactions_emit_verifiable_llvm() {
    let cases = [
        (
            "nested_values",
            r#"game "Nested Values"
struct Cell { value: i32, flags: [bool; 2] }
struct State { cells: [Cell; 2] }
let state: State = State { cells: [
    Cell { value: 1, flags: [true, false] },
    Cell { flags: [false, true], value: 2 }
] }
fn copy(value: State) -> State { return value }
start {
    let local: State = copy(state)
    local.cells[0].value = 9
    state.cells[1].value += 3
    if local.cells[0].flags[0] { print_i32(state.cells[1].value) }
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "forward_constants",
            r#"game "Forward Constants"
const WIDTH: i32 = SHAPE[0]
const GRID: [[i32; 2]; WIDTH] = [[1, 2], [3, 4]]
const SHAPE: [i32; 2] = [2, 2]
start { print_i32(GRID[1][0]) }
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "aggregate_rvalue",
            r#"game "Aggregate Rvalue"
struct Level { positions: [i32; 2] }
fn make() -> Level { return Level { positions: [7, 9] } }
fn index() -> i32 { return 1 }
start { print_i32(make().positions[index()]) }
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "shadowed_type_names",
            r#"game "Shadowed Type Names"
struct Limit {}
fn count(Limit: i32) -> i32 {
    let total: i32 = 0
    for Limit in 0..Limit { total += Limit }
    return total
}
start {
    let Limit: i32 = 3
    if Limit > 0 { print_i32(count(Limit)) }
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "whole_aggregate_assignment",
            r#"game "Whole Aggregate Assignment"
struct Point { x: i32, y: i32 }
let points: [Point; 2] = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }]
start {
    let point: Point = points[0]
    point = points[1]
    points[0] = point
    print_i32(points[0].y)
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "scalar_control",
            r#"game "Scalar Control"
fn choose(flag: bool, left: f32, right: f32) -> f32 {
    if flag { return left }
    else { return right }
}
start {
    let selected: f32 = choose(true, f32(4), 2.0)
    let integer: i32 = i32(selected)
    if integer >= 4 && integer != 0 { print_i32(integer) }
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "nested_arrays",
            r#"game "Nested Arrays"
let matrix: [[i32; 2]; 2] = [[1, 2], [3, 4]]
start {
    let row: i32 = 1
    let column: i32 = 0
    matrix[row][column] *= 2
    print_i32(matrix[row][column])
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "evaluated_once",
            r#"game "Evaluated Once"
let calls: i32 = 0
fn once() -> i32 { calls += 1 return 2 }
start {
    let value: i32 = 8
    value /= once()
    if false && once() == 2 { print_i32(99) }
    if true || once() == 2 { print_i32(value) }
    print_i32(calls)
}
update(dt: f32) {}
draw {}
"#,
        ),
        (
            "control_character_title",
            "game \"Control\0Title\"\nstart {}\nupdate(dt: f32) {}\ndraw {}\n",
        ),
    ];

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/semantic_contract_ir");
    fs::create_dir_all(&work).expect("semantic contract IR directory should exist");

    for (name, source) in cases {
        let ir = speck::compile_to_llvm(source).unwrap_or_else(|diagnostics| {
            panic!(
                "accepted case {name} failed:\n{}",
                speck::render_diagnostics(Path::new(name), source, &diagnostics)
            )
        });
        let path = work.join(format!("{name}.ll"));
        fs::write(&path, ir).expect("semantic contract IR should write");
        let verify = Command::new("llvm-as")
            .arg(&path)
            .arg("-o")
            .arg(work.join(format!("{name}.bc")))
            .output()
            .expect("llvm-as should start");
        assert_success(name, &verify);
    }
}

#[test]
fn invalid_feature_interactions_are_rejected_with_safe_spans() {
    let cases = [
        (
            "rvalue assignment",
            r#"game "Bad"
struct Point { x: i32 }
fn make() -> Point { return Point { x: 1 } }
start { make().x = 2 }
update(dt: f32) {}
draw {}
"#,
            "invalid assignment target",
        ),
        (
            "constant nested mutation",
            r#"game "Bad"
struct State { values: [i32; 1] }
const STATE: State = State { values: [1] }
start { STATE.values[0] += 1 }
update(dt: f32) {}
draw {}
"#,
            "cannot use compound assignment through constant `STATE`",
        ),
        (
            "array parameter",
            r#"game "Bad"
fn first(values: [i32; 1]) -> i32 { return values[0] }
start {}
update(dt: f32) {}
draw {}
"#,
            "arrays are not supported as function parameters yet",
        ),
        (
            "recursive layout",
            r#"game "Bad"
struct Node { children: [Node; 1] }
start {}
update(dt: f32) {}
draw {}
"#,
            "recursive value type is not supported",
        ),
        (
            "aggregate equality",
            r#"game "Bad"
struct Point { x: i32 }
start { let equal: bool = Point { x: 1 } == Point { x: 1 } }
update(dt: f32) {}
draw {}
"#,
            "equality comparison requires scalar operands",
        ),
        (
            "void value",
            r#"game "Bad"
fn effect() -> void {}
start { let value: i32 = effect() }
update(dt: f32) {}
draw {}
"#,
            "variable initializer requires a value, but found `void`",
        ),
        (
            "wrong index type",
            r#"game "Bad"
let values: [i32; 1] = [1]
start { print_i32(values[0.0]) }
update(dt: f32) {}
draw {}
"#,
            "array index must be i32",
        ),
        (
            "loop mutation",
            r#"game "Bad"
start { for i in 0..2 { i = 1 } }
update(dt: f32) {}
draw {}
"#,
            "loop variable `i` is read-only",
        ),
        (
            "missing return",
            r#"game "Bad"
fn value(flag: bool) -> i32 { if flag { return 1 } }
start {}
update(dt: f32) {}
draw {}
"#,
            "may finish without returning `i32`",
        ),
        (
            "nested initializer mismatch",
            r#"game "Bad"
struct State { values: [i32; 2] }
let state: State = State { values: [1] }
start {}
update(dt: f32) {}
draw {}
"#,
            "expected array length 2, found 1 elements",
        ),
        (
            "scalar index base",
            r#"game "Bad"
start { let value: i32 = 1 print_i32(value[0]) }
update(dt: f32) {}
draw {}
"#,
            "cannot index value of type `i32`",
        ),
        (
            "unknown field",
            r#"game "Bad"
struct Point { x: i32 }
start { let point: Point = Point { x: 1 } print_i32(point.y) }
update(dt: f32) {}
draw {}
"#,
            "type `Point` has no field named `y`",
        ),
        (
            "unknown call",
            r#"game "Bad"
start { missing() }
update(dt: f32) {}
draw {}
"#,
            "unknown function `missing`",
        ),
        (
            "invalid conversion",
            r#"game "Bad"
start { let value: i32 = i32(true) }
update(dt: f32) {}
draw {}
"#,
            "cannot convert `bool` to a numeric type",
        ),
        (
            "untyped array literal",
            r#"game "Bad"
start { print_i32([1]) }
update(dt: f32) {}
draw {}
"#,
            "array literal requires an explicit array type annotation",
        ),
        (
            "invalid unary operand",
            r#"game "Bad"
start { let value: i32 = -true }
update(dt: f32) {}
draw {}
"#,
            "unary `-` requires `i32` or `f32`",
        ),
    ];

    for (name, source, expected) in cases {
        let diagnostics = speck::analyze(source).expect_err(name);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "{name}: expected {expected:?}, found {diagnostics:#?}"
        );
        for diagnostic in &diagnostics {
            assert!(diagnostic.span.start <= diagnostic.span.end);
            assert!(diagnostic.span.end <= source.len());
            assert!(source.is_char_boundary(diagnostic.span.start));
            assert!(source.is_char_boundary(diagnostic.span.end));
        }
    }
}

#[test]
fn aggregate_copy_mutation_and_side_effects_match_the_value_model() {
    let output = run_source(
        "semantic_value_oracle",
        r#"game "Semantic Value Oracle"
struct Pair { values: [i32; 2] }
let calls: i32 = 0
let state: [Pair; 1] = [Pair { values: [1, 2] }]
fn once() -> i32 { calls += 1 return 3 }
fn changed(pair: Pair) -> Pair {
    pair.values[0] += 100
    return pair
}
start {
    let local: Pair = changed(state[0])
    print_i32(local.values[0])
    print_i32(state[0].values[0])
    state[0].values[1] += once()
    print_i32(calls)
    print_i32(state[0].values[1])
    let sum: i32 = 0
    for i in 1..4 { sum += i }
    print_i32(sum)
    print_i32(21 / calls)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_success("semantic value oracle", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "101\n1\n1\n5\n6\n21\n"
    );
}

#[test]
fn invalid_runtime_i32_division_terminates_before_sdiv() {
    let zero_source = r#"game "Division Zero"
fn dividend() -> i32 { print_i32(1) return 10 }
fn divisor() -> i32 { print_i32(2) return 0 }
start { print_i32(dividend() / divisor()) }
update(dt: f32) {}
draw {}
"#;
    let overflow_source = r#"game "Division Overflow"
let dividend: i32 = i32(-2147483648.0)
let divisor: i32 = -1
start { print_i32(dividend / divisor) }
update(dt: f32) {}
draw {}
"#;

    let ir = speck::compile_to_llvm(zero_source).expect("checked division should compile");
    let failure = ir
        .find("\ndivision_failure")
        .expect("division failure block should exist");
    let call = ir
        .find("call void @crumb_division_fail")
        .expect("division failure hook should be called");
    let valid = ir
        .find("\ndivision_valid")
        .expect("division valid block should exist");
    let divide = ir.find(" = sdiv i32").expect("valid sdiv should exist");
    assert!(failure < call && call < valid && valid < divide);

    let zero = run_source("division_zero", zero_source);
    assert!(!zero.status.success());
    assert_eq!(
        String::from_utf8_lossy(&zero.stderr),
        "Speck integer division by zero: 10 / 0\n"
    );
    assert_eq!(String::from_utf8_lossy(&zero.stdout), "1\n2\n");

    let overflow = run_source("division_overflow", overflow_source);
    assert!(!overflow.status.success());
    assert_eq!(
        String::from_utf8_lossy(&overflow.stderr),
        "Speck integer division overflow: -2147483648 / -1\n"
    );
    assert!(overflow.stdout.is_empty());

    let short_circuit = run_source(
        "division_short_circuit",
        r#"game "Division Short Circuit"
let zero: i32 = 0
start {
    if false && 10 / zero == 0 { print_i32(99) }
    print_i32(1)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_success("short-circuited division", &short_circuit);
    assert_eq!(String::from_utf8_lossy(&short_circuit.stdout), "1\n");
}

#[test]
fn loop_local_storage_is_allocated_once_in_the_entry_block() {
    let source = r#"game "Loop Storage"
let iteration: i32 = 0
start {
    while iteration < 2 {
        let outer: i32 = iteration
        for index in 0..2 {
            let inner: i32 = outer + index
            print_i32(inner)
        }
        iteration += 1
    }
}
update(dt: f32) {}
draw {}
"#;
    let ir = speck::compile_to_llvm(source).expect("loop locals should compile");
    let start = ir
        .split("define void @spk_start")
        .nth(1)
        .expect("start function should be emitted")
        .split("\n}\n")
        .next()
        .expect("start function should have a body");
    let loop_condition = start
        .find("while_condition")
        .expect("while loop should be emitted");
    let allocas = start
        .match_indices("alloca")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(allocas.len(), 3);
    assert!(allocas.iter().all(|index| *index < loop_condition));

    let output = run_source("loop_storage", source);
    assert_success("loop local storage", &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "0\n1\n1\n2\n");
}

#[test]
fn float_negation_preserves_negative_zero_for_locals_and_constants() {
    let output = run_source(
        "negative_zero",
        r#"game "Negative Zero"
const NEGATIVE_ZERO: f32 = -0.0
start {
    let local: f32 = -0.0
    debug_frame(1, 1.0 / local)
    debug_frame(2, 1.0 / NEGATIVE_ZERO)
}
update(dt: f32) {}
draw {}
"#,
    );
    assert_success("negative zero", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "frame 1: -inf\nframe 2: -inf\n"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ir = fs::read_to_string(
        root.join("target/semantic_contract_native/negative_zero/build/negative_zero.ll"),
    )
    .expect("negative-zero IR should be inspectable");
    assert!(ir.contains("fneg float"));
}

fn run_source(name: &str, source: &str) -> Output {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/semantic_contract_native").join(name);
    fs::create_dir_all(&work).expect("semantic contract directory should exist");
    let source_path = work.join(format!("{name}.spk"));
    fs::write(&source_path, source).expect("semantic contract source should write");
    let executable = build_in(&work, &source_path);
    Command::new(executable)
        .current_dir(&work)
        .output()
        .expect("semantic contract executable should start")
}

fn build_in(work: &Path, source: &Path) -> PathBuf {
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(work)
        .args(["build"])
        .arg(source)
        .output()
        .expect("Speck compiler should start");
    assert_success("semantic contract build", &build);
    let stem = source.file_stem().expect("source should have a stem");
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
