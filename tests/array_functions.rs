pub mod support;

use std::fs;
use std::process::Command;

#[test]
fn array_arguments_and_returns_copy_values_and_evaluate_once_in_order() {
    check_native(
        r#"game "Array function values"
const COUNT: i32 = 2
let calls: i32 = 0
fn tick(label: i32) -> i32 { print_i32(label) calls += 1 return calls }
fn changed(values: [i32; COUNT]) -> [i32; COUNT] { values[0] += 10 return values }
fn combine(left: [i32; COUNT], right: [i32; COUNT]) -> [i32; COUNT] {
    return [left[0] + right[0], left[1] + right[1]]
}
fn made() -> [i32; COUNT] { return [tick(30), tick(40)] }
start {
    let original: [i32; COUNT] = [1, 2]
    let copy: [i32; COUNT] = changed(original)
    print_i32(original[0])
    print_i32(copy[0])
    let combined: [i32; COUNT] = combine([tick(10), tick(20)], made())
    print_i32(combined[0])
    print_i32(combined[1])
    print_i32(calls)
    print_i32(changed(copy)[0])
    print_i32(copy[0])
}
update(dt: f32) {}
draw {}
"#,
        "1\n11\n10\n20\n30\n40\n4\n6\n4\n21\n11\n",
    );
}

#[test]
fn nested_struct_arrays_and_boolean_arrays_cross_function_boundaries() {
    check_native(
        r#"game "Nested array values"
struct Cell { value: i32 }
fn changed(grid: [[Cell; 2]; 2]) -> [[Cell; 2]; 2] {
    grid[0][1].value += 7
    return grid
}
fn swapped(flags: [bool; 2]) -> [bool; 2] { return [flags[1], flags[0]] }
start {
    let original: [[Cell; 2]; 2] = [[Cell { value: 1 }, Cell { value: 2 }], [Cell { value: 3 }, Cell { value: 4 }]]
    let copy: [[Cell; 2]; 2] = changed(original)
    print_i32(original[0][1].value)
    print_i32(copy[0][1].value)
    print_i32(changed(copy)[1][0].value)
    let flags: [bool; 2] = swapped([true, false])
    if flags[0] { print_i32(-1) }
    if flags[1] { print_i32(1) }
}
update(dt: f32) {}
draw {}
"#,
        "2\n9\n3\n1\n",
    );
}

#[test]
fn returned_arrays_can_be_indexed_dynamically_inside_repeated_loops() {
    check_native(
        r#"game "Returned array storage"
fn row(value: i32) -> [i32; 4] { return [value, value + 1, value + 2, value + 3] }
fn forward(value: [i32; 4]) -> [i32; 4] { return value }
start {
    let index: i32 = 2
    let last: i32 = 0
    for i in 0..100000 { last = forward(row(i))[index] }
    print_i32(last)
}
update(dt: f32) {}
draw {}
"#,
        "100001\n",
    );
}

fn check_native(source: &str, expected: &str) {
    let directory = support::workspace();
    let work = directory.path();
    let path = work.join("array_functions.spk");
    fs::write(&path, source).unwrap();
    let executable = support::build_in(work, &path);
    support::assert_success(
        "array function IR verification",
        &support::verify_ir(
            &work.join("build/array_functions.ll"),
            &work.join("verified.bc"),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success("array function executable", &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
