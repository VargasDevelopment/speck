pub mod support;

use std::fs;
use std::process::Command;

use speck::ast::ConstantValue;

const ENTRIES: &str = "start {} update(dt: f32) {} draw {}";
const TYPES: &str = "struct Row { width: i32 } struct Config { rows: [Row; 2] }";

#[test]
fn aggregate_dependencies_agree_across_constants_lengths_and_globals() {
    let initializer = "Config { flags: [false && (1 / 0 == 0), true || (1 / 0 == 0)], rows: [Row { width: BASE }, Row { width: i32(f32(BASE)) + 1 }] }";
    let source = format!(
        r#"game "Shared constant semantics"
struct Row {{ width: i32 }}
struct Config {{ rows: [Row; ROW_COUNT], flags: [bool; 2] }}
const WIDTH: i32 = COPY.rows[1].width
const COPY: Config = CONFIG
const CONFIG: Config = {initializer}
const BASE: i32 = 2
const ROW_COUNT: i32 = 2
let literal: Config = {initializer}
let aliased: Config = COPY
let tiles: [i32; WIDTH] = [7, 8, 9]
start {{
    aliased.rows[1].width = 99
    print_i32(WIDTH)
    print_i32(CONFIG.rows[1].width)
    print_i32(literal.rows[1].width)
    print_i32(aliased.rows[1].width)
    print_i32(tiles[2])
    if literal.flags[0] {{ print_i32(-1) }}
    if literal.flags[1] {{ print_i32(1) }}
}}
update(dt: f32) {{}}
draw {{}}
"#
    );
    let program = speck::analyze(&source).expect("all evaluation contexts should agree");
    let program = program.ast();
    let constant = |name| {
        program
            .constants
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .value
            .as_ref()
            .unwrap()
    };
    assert_eq!(constant("WIDTH"), &ConstantValue::I32(3));
    assert_eq!(constant("COPY"), constant("CONFIG"));
    for global in program.globals.iter().filter(|item| item.name != "tiles") {
        assert_eq!(global.value.as_ref(), Some(constant("CONFIG")));
    }

    let directory = support::workspace();
    let work = directory.path();
    let path = work.join("constant_contexts.spk");
    fs::write(&path, source).unwrap();
    let executable = support::build_in(work, &path);
    support::assert_success(
        "independent constant IR verification",
        &support::verify_ir(
            &work.join("build/constant_contexts.ll"),
            &work.join("verified.bc"),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success("constant contexts executable", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n3\n3\n99\n9\n1\n"
    );
}

#[test]
fn malformed_aggregates_are_rejected_in_every_evaluation_context() {
    for (initializer, expected) in [
        (
            "Config { rows: [Row { width: 2 }] }",
            "expected array length 2, found 1 elements",
        ),
        (
            "Config { rows: [Row { width: 2 }, Row {}] }",
            "missing initializer for field `width`",
        ),
        (
            "Config { rows: [Row { width: 2 }, Row { width: 1 / 0 }] }",
            "division by zero in constant expression",
        ),
    ] {
        for (declaration, length_use) in [
            ("const", ""),
            ("let", ""),
            (
                "const",
                "const N: i32 = DATA.rows[0].width\nstruct UsesLength { values: [i32; N] }",
            ),
        ] {
            let source = format!(
                "game \"Invalid aggregate\"\n{TYPES}\n{declaration} DATA: Config = {initializer}\n{length_use}\n{ENTRIES}"
            );
            let errors = speck::analyze(&source).expect_err("invalid aggregate must fail");
            assert!(
                errors.iter().any(|error| error.message.contains(expected)),
                "{declaration}, {length_use}: {errors:#?}"
            );
            assert!(
                errors.iter().all(
                    |error| error.span.start < error.span.end && error.span.end <= source.len()
                ),
                "diagnostics need source spans: {errors:#?}"
            );
        }
    }
}

#[test]
fn aggregate_dependency_cycles_keep_phase_specific_paths() {
    for (length_use, expected) in [
        ("", "cyclic constant definition: DATA -> N -> DATA"),
        (
            "struct UsesLength { values: [i32; N] }",
            "cyclic array-length constant: N -> DATA -> N",
        ),
    ] {
        let source = format!(
            "game \"Aggregate cycle\"\n{TYPES}\nconst DATA: Config = Config {{ rows: [Row {{ width: N }}, Row {{ width: 2 }}] }}\nconst N: i32 = DATA.rows[0].width\n{length_use}\n{ENTRIES}"
        );
        let errors = speck::analyze(&source).expect_err("aggregate cycle must fail");
        assert_eq!(
            errors.len(),
            1,
            "cycle should have one root diagnostic: {errors:#?}"
        );
        assert_eq!(errors[0].message, expected);
    }
}

#[test]
fn short_circuited_aggregate_fields_still_reject_forbidden_dependencies() {
    for dependency in ["mutable > 0", "read() > 0"] {
        for declaration in ["const", "let"] {
            let source = format!(
                "game \"Forbidden dependency\"\nstruct Flags {{ enabled: bool }}\nlet mutable: i32 = 1\nfn read() -> i32 {{ return 1 }}\n{declaration} FLAGS: Flags = Flags {{ enabled: false && ({dependency}) }}\n{ENTRIES}"
            );
            let errors = speck::analyze(&source)
                .expect_err("short circuit does not permit mutable dependencies or calls");
            let context = if declaration == "const" {
                "constant expressions"
            } else {
                "global initializers"
            };
            let expected = if dependency.starts_with("mutable") {
                format!("{context} cannot reference mutable global `mutable`")
            } else {
                format!("{context} cannot call function `read`")
            };
            assert!(
                errors.iter().any(|error| error.message == expected),
                "{errors:#?}"
            );
        }
    }
}
