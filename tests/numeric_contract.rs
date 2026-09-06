pub mod support;

use std::fs;
use std::process::Command;

const ENTRIES: &str = "update(dt: f32) {} draw {}";
const LARGE_FLOAT: &str = "300000000000000000000000000000000000000.0";

#[test]
fn runtime_integer_arithmetic_wraps_and_division_truncates() {
    let cases = [
        ("2147483647 + 1", i32::MIN),
        ("-2147483648 - 1", i32::MAX),
        ("1073741824 * 2", i32::MIN),
        ("-(-2147483648)", i32::MIN),
        ("(2147483647 + 1) - 1", i32::MAX),
        ("-7 / 3", -2),
        ("7 / -3", -2),
        ("-7 / -3", 2),
        ("-2147483648 / 1", i32::MIN),
    ];
    let mut body = String::new();
    let mut expected = String::new();
    for (index, (expression, result)) in cases.into_iter().enumerate() {
        body.push_str(&format!(
            "let value{index}: i32 = {expression}\nprint_i32(value{index})\n"
        ));
        expected.push_str(&format!("{result}\n"));
    }
    body.push_str(
        "let add: i32 = 2147483647\nadd += 1\nprint_i32(add)\n\
         let subtract: i32 = -2147483648\nsubtract -= 1\nprint_i32(subtract)\n\
         let multiply: i32 = 1073741824\nmultiply *= 2\nprint_i32(multiply)\n",
    );
    expected.push_str("-2147483648\n2147483647\n-2147483648\n");
    assert_native_output(&format!("start {{ {body} }}"), &expected);
}

#[test]
fn top_level_initializers_reject_overflow_and_nonfinite_intermediates() {
    let float_overflow = format!("{LARGE_FLOAT} * 2.0");
    let cases = [
        ("i32", "2147483647 + 1", "constant-expression overflow"),
        ("i32", "-2147483648 - 1", "constant-expression overflow"),
        ("i32", "1073741824 * 2", "constant-expression overflow"),
        ("i32", "-(-2147483648)", "constant-expression overflow"),
        ("i32", "-2147483648 / -1", "constant-expression overflow"),
        (
            "i32",
            "(2147483647 + 1) - 1",
            "constant-expression overflow",
        ),
        (
            "f32",
            float_overflow.as_str(),
            "constant-expression overflow",
        ),
        (
            "f32",
            "1.0 / -0.0",
            "division by zero in constant expression",
        ),
        (
            "i32",
            "i32(1.0 / 0.0)",
            "division by zero in constant expression",
        ),
    ];
    for declaration in ["const", "let"] {
        for (ty, expression, diagnostic) in cases {
            assert_rejected(
                &format!("{declaration} value: {ty} = {expression}\nstart {{}}"),
                diagnostic,
            );
        }
    }
}

#[test]
fn out_of_range_literals_are_rejected_even_in_runtime_expressions() {
    for (ty, expression, diagnostic) in [
        ("i32", "2147483648", "integer literal does not fit in `i32`"),
        (
            "i32",
            "-2147483649",
            "integer literal does not fit in `i32`",
        ),
        (
            "f32",
            "400000000000000000000000000000000000000.0",
            "must be finite",
        ),
    ] {
        for declaration in ["const", "let"] {
            assert_rejected(
                &format!("{declaration} value: {ty} = {expression}\nstart {{}}"),
                diagnostic,
            );
        }
        assert_rejected(
            &format!("start {{ let value: {ty} = {expression} }}"),
            diagnostic,
        );
    }
}

#[test]
fn conversion_rounding_agrees_for_constants_globals_and_runtime_values() {
    let cases = [
        ("i32(2147483520.0)", 2147483520), // Last f32 below the upper clamp.
        ("i32(-2147483520.0)", -2147483520),
        ("i32(2147483904.0)", i32::MAX),
        ("i32(-2147483904.0)", i32::MIN),
        ("i32(f32(16777217))", 16777216), // Halfway: round to even.
        ("i32(f32(16777219))", 16777220),
        ("i32(f32(-16777217))", -16777216),
        ("i32(f32(2147483647))", i32::MAX),
        ("i32(f32(-2147483648))", i32::MIN),
    ];
    let mut declarations = String::new();
    let mut body = String::new();
    let mut expected = String::new();
    for (index, (expression, result)) in cases.into_iter().enumerate() {
        declarations.push_str(&format!(
            "const C{index}: i32 = {expression}\nlet g{index}: i32 = {expression}\n"
        ));
        body.push_str(&format!(
            "print_i32(C{index})\nprint_i32(g{index})\nprint_i32({expression})\n"
        ));
        expected.push_str(&format!("{result}\n{result}\n{result}\n"));
    }
    assert_native_output(&format!("{declarations}\nstart {{ {body} }}"), &expected);
}

#[test]
fn runtime_nonfinite_values_clamp_and_all_float_comparisons_are_ordered() {
    let mut body = format!(
        "let nan: f32 = zero / zero\n\
         let positive: f32 = 1.0 / zero\n\
         let negative: f32 = -1.0 / zero\n\
         let overflow: f32 = {LARGE_FLOAT} * 2.0\n\
         print_i32(i32(positive))\nprint_i32(i32(negative))\n\
         print_i32(i32(overflow))\n"
    );
    let mut expected = "2147483647\n-2147483648\n2147483647\n".to_owned();
    // Finite controls ensure that a constant-false comparison implementation
    // cannot satisfy the NaN cases. Include NaN in both operand positions.
    for (left, right, results) in [
        ("nan", "zero", [false; 6]),
        ("zero", "nan", [false; 6]),
        ("nan", "nan", [false; 6]),
        ("1.0", "2.0", [false, true, true, true, false, false]),
        ("2.0", "1.0", [false, true, false, false, true, true]),
        ("zero", "-zero", [true, false, false, true, false, true]),
    ] {
        for (operator, result) in ["==", "!=", "<", "<=", ">", ">="].into_iter().zip(results) {
            body.push_str(&format!("print_bool({left} {operator} {right})\n"));
            expected.push_str(if result { "1\n" } else { "0\n" });
        }
    }
    assert_native_output(
        &format!(
            "let zero: f32 = 0.0\n\
             fn print_bool(value: bool) -> void {{\n\
                 if value {{ print_i32(1) }} else {{ print_i32(0) }}\n\
             }}\nstart {{ {body} }}"
        ),
        &expected,
    );
}

fn assert_rejected(declarations: &str, expected: &str) {
    let source = format!("game \"Numeric rejection\"\n{declarations}\n{ENTRIES}");
    let errors = speck::analyze(&source).expect_err(&source);
    assert!(
        errors.iter().any(|error| error.message.contains(expected)),
        "{source}\nexpected {expected:?}, found {errors:#?}"
    );
}

fn assert_native_output(declarations: &str, expected: &str) {
    let directory = support::workspace();
    let work = directory.path();
    let source_path = work.join("numeric_contract.spk");
    let source = format!("game \"Numeric contract\"\n{declarations}\n{ENTRIES}");
    fs::write(&source_path, &source).unwrap();
    let executable = support::build_in(work, &source_path);
    support::assert_success(
        "independent numeric IR verification",
        &support::verify_ir(
            &work.join("build/numeric_contract.ll"),
            &work.join("verified.bc"),
        ),
    );
    let output = support::run(Command::new(executable).current_dir(work));
    support::assert_success("numeric contract executable", &output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected,
        "{source}"
    );
}
