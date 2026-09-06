use std::process::Command;
use std::time::{Duration, Instant};

const ENTRIES: &str = "start {}\nupdate(dt: f32) {}\ndraw {}\n";

#[test]
fn shared_struct_graphs_finish_within_budget() {
    const CHILD: &str = "SPECK_STRUCT_GRAPH_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let mut declarations = String::from("struct T0 { value: i32 }\n");
        for level in 1..=40 {
            declarations.push_str(&format!(
                "struct T{level} {{ left: T{previous}, right: [T{previous}; 1] }}\n",
                previous = level - 1,
            ));
        }
        let source = format!("game \"Shared graph\"\n{declarations}{ENTRIES}");
        speck::analyze(&source).expect("a shared acyclic graph is valid");

        // A mismatched initializer forces type validation without materializing
        // an exponentially large value. Exercise both consumers of that check.
        for declaration in ["const VALUE: T40 = 0", "let value: [T40; 1] = 0"] {
            let source = format!("{source}\n{declaration}\n");
            let errors = speck::analyze(&source).expect_err("initializer has the wrong type");
            assert!(
                errors
                    .iter()
                    .any(|error| error.message.contains("initializer"))
            );
        }

        // Forward references force a deep graph traversal, but the source types
        // themselves are shallow. Graph depth must not consume the call stack.
        let mut source = String::from("game \"Long chain\"\n");
        for level in 0..10_000 {
            source.push_str(&format!("struct T{level} {{ next: T{} }}\n", level + 1));
        }
        source.push_str("struct T10000 { value: i32 }\n");
        source.push_str(ENTRIES);
        speck::analyze(&source).expect("a long acyclic declaration chain is valid");
        return;
    }

    // Isolate the compiler so an exponential-time regression cannot hang the suite.
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "shared_struct_graphs_finish_within_budget",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .spawn()
        .expect("spawn graph regression test");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll graph regression test") {
            assert!(status.success(), "graph regression child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().expect("stop timed-out graph regression test");
            child.wait().expect("reap graph regression test");
            panic!("shared struct graph analysis exceeded ten seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cycle_diagnostics_identify_all_and_only_cycle_members_in_source_order() {
    let source = format!(
        "game \"Cycles\"\n\
         struct Wrapper {{ value: A }}\n\
         struct Z {{ next: [Z; 1] }}\n\
         struct A {{ first: B, second: C }}\n\
         struct B {{ back: A }}\n\
         struct Leaf {{ value: i32 }}\n\
         struct C {{ back: [A; 2], leaf: Leaf }}\n\
         struct D {{ next: E }}\n\
         struct E {{ next: D }}\n{ENTRIES}"
    );
    let errors = speck::analyze(&source).expect_err("value cycles are invalid");
    let expected = ["Z", "A", "B", "C", "D", "E"];
    assert_eq!(errors.len(), expected.len(), "{errors:#?}");
    for (error, name) in errors.iter().zip(expected) {
        assert_eq!(
            error.message,
            format!("recursive value type is not supported: struct `{name}` contains itself")
        );
        assert!(source[error.span.start..error.span.end].starts_with(&format!("struct {name} ")));
    }
}

#[test]
fn unknown_and_duplicate_types_keep_their_declaration_errors() {
    let source = format!(
        "game \"Declarations\"\n\
         struct A {{ recursive: A }}\n\
         struct A {{ missing: [Missing; 2] }}\n{ENTRIES}"
    );
    let errors = speck::analyze(&source).expect_err("declarations are invalid");
    let messages: Vec<_> = errors.iter().map(|error| error.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "struct `A` is declared more than once",
            "unknown struct type `Missing`"
        ]
    );
}

#[test]
fn invalid_array_lengths_propagate_through_shared_and_cyclic_structs() {
    for fields in ["other: Leaf", "other: [Root; 1]"] {
        let source = format!(
            "game \"Invalid lengths\"\n\
             struct Root {{ left: Branch, right: [Branch; 1] }}\n\
             struct Branch {{ value: Leaf, {fields} }}\n\
             struct Leaf {{ values: [i32; 0] }}\n\
             const VALUE: Root = 0\n\
             let value: [Root; 1] = 0\n{ENTRIES}"
        );
        let errors = speck::analyze(&source).expect_err("array length is invalid");
        assert_eq!(
            errors
                .iter()
                .filter(|e| e.message.contains("array length must be positive"))
                .count(),
            1
        );
        assert!(
            errors
                .iter()
                .all(|e| e.message.contains("array length must be positive")
                    || e.message.contains("recursive value type")),
            "unexpected cascading diagnostics: {errors:#?}"
        );
    }
}
