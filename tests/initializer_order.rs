pub mod support;

use std::fs;
use std::process::Command;

#[test]
fn reordered_fields_execute_in_source_order_and_keep_declared_layout() {
    check_native(
        r#"game "Field effects"
struct Pair { a: i32 b: i32 }
let calls: i32 = 0
fn tick(label: i32) -> i32 { print_i32(label) calls += 1 return calls }
fn consume(pair: Pair) -> void { print_i32(pair.a) print_i32(pair.b) }
fn make() -> Pair { return Pair { b: tick(20), a: tick(10) } }
start { consume(make()) print_i32(calls) }
update(dt: f32) {}
draw {}
"#,
        "20\n10\n2\n1\n2\n",
    );
}

#[test]
fn nested_array_and_short_circuit_fields_finish_before_the_next_field() {
    check_native(
        r#"game "Nested field effects"
struct Pair { a: i32 b: i32 }
struct Container { pairs: [Pair; 2] enabled: bool tail: i32 }
let calls: i32 = 0
fn tick(label: i32) -> i32 { print_i32(label) calls += 1 return calls }
start {
    let value: Container = Container {
        tail: tick(90),
        enabled: tick(70) > 0 && tick(80) > 0,
        pairs: [Pair { b: tick(20), a: tick(10) }, Pair { a: tick(30), b: tick(40) }]
    }
    print_i32(value.tail)
    if value.enabled { print_i32(1) }
    print_i32(value.pairs[0].a)
    print_i32(value.pairs[0].b)
    print_i32(value.pairs[1].a)
    print_i32(value.pairs[1].b)
    print_i32(calls)
}
update(dt: f32) {}
draw {}
"#,
        "90\n70\n80\n20\n10\n30\n40\n1\n1\n5\n4\n6\n7\n7\n",
    );
}

fn check_native(source: &str, expected: &str) {
    let directory = support::workspace();
    let work = directory.path();
    let path = work.join("initializer_order.spk");
    fs::write(&path, source).unwrap();
    let executable = support::build_in(work, &path);
    support::assert_success(
        "initializer order IR verification",
        &support::verify_ir(
            &work.join("build/initializer_order.ll"),
            &work.join("verified.bc"),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success("initializer order executable", &output);
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
