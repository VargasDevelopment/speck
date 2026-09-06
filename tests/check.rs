pub mod support;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const VALID_SOURCE: &str = "game \"Checked\"\nstart {}\nupdate(dt: f32) {}\ndraw {}\n";

// Cargo builds the test binary using the normal development toolchain; only
// the checked command receives an empty PATH, so it cannot discover tools.
fn check_command(work: &Path, args: &[&str]) -> Output {
    support::run(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work)
            .env("PATH", "")
            .args(args),
    )
}

#[test]
fn valid_source_checks_without_native_tools_or_artifacts() {
    let directory = support::workspace();
    let work = directory.path();
    fs::write(work.join("game.spk"), VALID_SOURCE).unwrap();

    let output = check_command(work, &["check", "game.spk"]);
    support::assert_success("source check without native tools", &output);
    assert_eq!(output.stdout, b"Checked: game.spk\n");
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_dir(work).unwrap().count(), 1);
    assert_eq!(
        fs::read_to_string(work.join("game.spk")).unwrap(),
        VALID_SOURCE
    );
}

#[test]
fn check_leaves_existing_build_artifacts_untouched() {
    let directory = support::workspace();
    let work = directory.path();
    fs::write(work.join("game.spk"), VALID_SOURCE).unwrap();
    fs::create_dir(work.join("build")).unwrap();
    for name in ["game", "game.ll", "game.bc", "frame.ppm"] {
        fs::write(work.join("build").join(name), name).unwrap();
    }

    let output = check_command(work, &["check", "game.spk"]);
    support::assert_success("source check with existing build artifacts", &output);
    assert_eq!(fs::read_dir(work).unwrap().count(), 2);
    assert_eq!(fs::read_dir(work.join("build")).unwrap().count(), 4);
    for name in ["game", "game.ll", "game.bc", "frame.ppm"] {
        assert_eq!(
            fs::read_to_string(work.join("build").join(name)).unwrap(),
            name
        );
    }
}

#[test]
fn check_reports_all_analysis_stages_without_native_tools() {
    let directory = support::workspace();
    let work = directory.path();
    for source in [
        VALID_SOURCE.replace("start {}", "start { @ }"),
        VALID_SOURCE.replace("start {}", "start { let }"),
        VALID_SOURCE.replace("start {}", "start { print_i32(true) }"),
    ] {
        let diagnostics = speck::analyze(&source).expect_err("invalid fixture");
        let expected = speck::render_diagnostics(Path::new("game.spk"), &source, &diagnostics);
        fs::write(work.join("game.spk"), &source).unwrap();
        let output = check_command(work, &["check", "game.spk"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("{expected}\n")
        );
        assert_eq!(fs::read_dir(work).unwrap().count(), 1);
    }
}

#[test]
fn check_rejects_invalid_arguments_and_unreadable_sources() {
    let directory = support::workspace();
    let work = directory.path();
    for args in [
        vec!["check"],
        vec!["check", "first.spk", "second.spk"],
        vec!["check", "game.spk", "--frames", "1"],
        vec!["check", "game.txt"],
        vec!["check", "--unknown"],
    ] {
        let output = check_command(work, &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
    }
    let output = check_command(work, &["check", "missing.spk"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not read `missing.spk`"));

    fs::write(work.join("invalid_utf8.spk"), [0xff]).unwrap();
    let output = check_command(work, &["check", "invalid_utf8.spk"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("could not read `invalid_utf8.spk`"));
    assert_eq!(fs::read_dir(work).unwrap().count(), 1);
}

#[test]
fn help_lists_check_and_existing_modes() {
    let directory = support::workspace();
    for flag in ["--help", "-h"] {
        let output = check_command(directory.path(), &[flag]);
        support::assert_success("CLI help", &output);
        let help = String::from_utf8(output.stdout).unwrap();
        for command in ["check", "build", "dev", "run"] {
            assert!(help.contains(&format!("speck {command} <game.spk>")));
        }
        assert!(output.stderr.is_empty());
    }
}
