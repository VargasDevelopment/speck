pub mod support;

use std::fs;
use std::process::Command;

#[test]
fn sine_matches_known_values_and_propagates_nonfinite_inputs() {
    let work = support::workspace();
    let source = work.path().join("sine.spk");
    fs::write(
        &source,
        r#"game "Sine"
fn near(actual: f32, expected: f32) -> bool {
    return actual > expected - 0.00001 && actual < expected + 0.00001
}
fn nan(value: f32) -> bool { return !(value <= 0.0 || value > 0.0) }
start {
    if sin(0.0) == 0.0 { print_i32(1) }
    if near(sin(1.5707964), 1.0) { print_i32(2) }
    if near(sin(-1.5707964), -1.0) { print_i32(3) }
    if near(sin(3.1415927), 0.0) { print_i32(4) }
    if near(sin(0.5235988), 0.5) { print_i32(5) }
    if near(sin(1000000.0), -0.3499935) { print_i32(6) }
    let infinity: f32 = 1.0 / 0.0
    let not_number: f32 = 0.0 / 0.0
    if nan(sin(infinity)) && nan(sin(-infinity)) && nan(sin(not_number)) { print_i32(7) }
    let negative_zero: f32 = sin(-0.0)
    if 1.0 / negative_zero < 0.0 { print_i32(8) }
}
update(dt: f32) {} draw {}"#,
    )
    .unwrap();
    let executable = support::build_in(work.path(), &source);
    let verified = support::verify_ir(
        &work.path().join("build/sine.ll"),
        &work.path().join("verified.bc"),
    );
    support::assert_success("independent sine LLVM verification", &verified);
    let output = support::run(Command::new(executable).current_dir(work.path()));
    support::assert_success("native sine values", &output);
    assert_eq!(output.stdout, b"1\n2\n3\n4\n5\n6\n7\n8\n");
}

#[test]
fn sine_uses_the_existing_builtin_type_and_constant_boundaries() {
    for declaration in [
        "start { let x: f32 = sin(1) }",
        "start { let x: f32 = sin(true) }",
        "start { let x: i32 = sin(1.0) }",
        "start { sin() }",
        "start { sin(1.0, 2.0) }",
        "fn sin(x: f32) -> f32 { return x } start {}",
        "const X: f32 = sin(1.0) start {}",
        "let x: f32 = sin(1.0) start {}",
    ] {
        let source = format!("game \"Invalid sine\" {declaration} update(dt: f32) {{}} draw {{}}");
        assert!(speck::analyze(&source).is_err(), "accepted {declaration}");
    }
}

// The separate libm/libc sonames below are a glibc contract. The native
// numerical test above also runs on Linux targets whose libc includes math.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn gnu_linux_keeps_the_math_library_only_for_sine_callers() {
    for (body, uses_sine) in [("print_i32(0)", false), ("print_i32(i32(sin(1.0)))", true)] {
        let work = support::workspace();
        let source = work.path().join("dependencies.spk");
        fs::write(
            &source,
            format!("game \"Dependencies\" start {{ {body} }} update(dt: f32) {{}} draw {{}}"),
        )
        .unwrap();
        let executable = support::build_in(work.path(), &source);
        let output = support::run(Command::new("readelf").arg("--dynamic").arg(executable));
        support::assert_success("ELF dynamic dependencies", &output);
        let dependencies = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            dependencies.contains("libm.so.6"),
            uses_sine,
            "{dependencies}"
        );
        assert!(dependencies.contains("libc.so.6"), "{dependencies}");
    }
}
