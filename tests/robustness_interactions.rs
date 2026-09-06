pub mod support;

use std::fs;
use std::process::Command;

#[test]
fn nested_short_circuit_joins_feed_loop_conditions_and_returns() {
    assert_oracle(
        "short_circuit_control",
        r#"game "Short Circuit Control"
let calls: i32 = 0
fn probe(value: i32) -> bool { calls += 1 return value < 3 }
fn total() -> i32 {
    let i: i32 = 0
    let sum: i32 = 0
    while probe(i) && (i == 0 || probe(i + 1)) {
        sum += i
        i += 1
    }
    if probe(4) || (probe(1) && !probe(5)) { return sum + 10 }
    return -1
}
start {
    print_i32(total())
    print_i32(calls)
}
update(dt: f32) {}
draw {}
"#,
        // Loop bodies run for 0 and 1. Five probes reach loop exit,
        // then three more probes select the early return.
        "11\n8\n",
    );
}

#[test]
fn checked_nested_compound_targets_evaluate_indices_and_rhs_once() {
    assert_oracle(
        "compound_indices",
        r#"game "Compound Indices"
struct Row { values: [i32; 2] }
let rows: [Row; 2] = [Row { values: [9, 8] }, Row { values: [7, 17] }]
let row_calls: i32 = 0
let column_calls: i32 = 0
let rhs_calls: i32 = 0
fn row() -> i32 { row_calls += 1 return 1 }
fn column() -> i32 { column_calls += 1 return 1 }
fn divisor() -> i32 { rhs_calls += 1 return 5 }
start {
    rows[row()].values[column()] %= divisor()
    print_i32(rows[1].values[1])
    print_i32(rows[1].values[0])
    print_i32(rows[0].values[1])
    print_i32(row_calls)
    print_i32(column_calls)
    print_i32(rhs_calls)
}
update(dt: f32) {}
draw {}
"#,
        // Only the selected slot changes, and both checked indices and
        // the checked-remainder divisor are each evaluated once.
        "2\n7\n8\n1\n1\n1\n",
    );
}

#[test]
fn aggregate_returns_cross_loop_exits_before_dynamic_rvalue_indexing() {
    assert_oracle(
        "aggregate_loop_return",
        r#"game "Aggregate Loop Return"
struct Pair { values: [i32; 2] }
const BASE: Pair = Pair { values: [1, 2] }
let index_calls: i32 = 0
fn index() -> i32 { index_calls += 1 return 1 }
fn changed(value: Pair, flag: bool) -> Pair {
    if flag {
        value.values[1] += 10
        return value
    }
    value.values[0] += 20
    return value
}
fn find() -> Pair {
    for i in 0..3 {
        let local: Pair = changed(BASE, i == 1)
        if i > 0 && local.values[i % 2] > 10 { return local }
    }
    return BASE
}
start {
    print_i32(find().values[index()])
    print_i32(index_calls)
    print_i32(BASE.values[0])
    print_i32(BASE.values[1])
}
update(dt: f32) {}
draw {}
"#,
        // The second iteration returns its modified copy through two
        // function boundaries; indexing that rvalue leaves BASE untouched.
        "12\n1\n1\n2\n",
    );
}

fn assert_oracle(name: &str, source: &str, expected: &str) {
    // Only this explicit terminating corpus is executed. Build, verification,
    // and execution each use the shared subprocess deadlines and workspace.
    let directory = support::workspace();
    let work = directory.path();
    let path = work.join(format!("{name}.spk"));
    fs::write(&path, source).expect("interaction source should write");
    let executable = support::build_in(work, &path);
    support::assert_success(
        &format!("{name} LLVM verification"),
        &support::verify_ir(
            &work.join(format!("build/{name}.ll")),
            &work.join(format!("{name}.bc")),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success(name, &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{name}");
    assert!(output.stderr.is_empty(), "{name}: unexpected stderr");
}
