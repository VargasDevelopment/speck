pub mod support;

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

#[test]
fn development_guards_report_the_original_imported_expression() {
    let cases = [
        ("print_i32(values[index])", "values[index]", "array index"),
        ("values[index] = 9", "values[index]", "array index"),
        ("values[index] += 1", "values[index]", "array index"),
        ("print_i32(pair()[index])", "pair()[index]", "array index"),
        ("print_i32(12 / zero)", "12 / zero", "division"),
        ("print_i32(12 % zero)", "12 % zero", "remainder"),
        (
            "print_i32(-2147483648 / index)",
            "-2147483648 / index",
            "division",
        ),
        (
            "print_i32(-2147483648 % index)",
            "-2147483648 % index",
            "remainder",
        ),
        ("index /= zero", "index", "division"),
        ("index %= zero", "index", "remainder"),
    ];
    for (operation, location, reason) in cases {
        let work = support::workspace();
        let imported = work.path().join("café \"quoted\"\\name::source.spk");
        let line = format!("    {operation}");
        fs::write(&imported, format!(
            "fn fail(index: i32, zero: i32) -> void {{\n    let values: [i32; 2] = [3, 4]\n{line}\n}}\nfn pair() -> [i32; 2] {{ return [1, 2] }}\n"
        )).unwrap();
        symlink(&imported, work.path().join("library.spk")).unwrap();
        let entry = work.path().join("game.spk");
        fs::write(&entry, "game \"Located\"\nimport \"library.spk\" as lib\nstart { lib::fail(-1, 0) } update(dt: f32) {} draw {}\n").unwrap();
        let output = support::run(
            Command::new(env!("CARGO_BIN_EXE_speck"))
                .current_dir(work.path())
                .arg("dev")
                .arg(&entry)
                .args(["--port", "0"]),
        );
        assert!(!output.status.success(), "guard did not fail: {operation}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let expected = format!(
            "{}:3:{}:",
            imported.canonicalize().unwrap().display(),
            line.find(location).unwrap() + 1
        );
        assert!(
            stderr.contains(&expected),
            "{operation}: expected {expected:?}, got {stderr}"
        );
        assert!(stderr.contains(reason), "{operation}: {stderr}");
        support::assert_success(
            "located LLVM verification",
            &support::verify_ir(
                &work.path().join("build/game_dev.ll"),
                &work.path().join("verified.bc"),
            ),
        );
    }
}

#[test]
fn ordinary_emission_omits_locations_while_development_maps_entry_source() {
    let work = support::workspace();
    let path = work.path().join("entry.spk");
    fs::write(&path, "game \"Entry\"\nfn fail(value: i32) -> i32 { return 1 / value }\nstart { print_i32(fail(0)) } update(dt: f32) {} draw {}\n").unwrap();
    let checked = speck::analyze_path(&path).unwrap();
    let ordinary = speck::codegen::llvm::emit(&checked);
    assert!(!ordinary.contains("@spk_source_"));
    assert!(!ordinary.contains("@crumb_source_location"));
    let development = speck::codegen::llvm::emit_for_development(&checked, None);
    assert!(development.contains("@crumb_source_location"));
    let executable = support::build_in(work.path(), &path);
    let output = support::run(Command::new(executable).current_dir(work.path()));
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("division"));
    assert!(
        !stderr.contains("entry.spk"),
        "ordinary build retained source location: {stderr}"
    );
    let output = support::run(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work.path())
            .arg("dev")
            .arg(&path)
            .args(["--port", "0"]),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains(&format!("{}:2:37:", path.display())),
        "{stderr}"
    );
}
