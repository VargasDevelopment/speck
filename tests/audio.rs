pub mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::assert_success;

#[test]
fn portable_audio_is_bounded_deterministic_and_thread_safe() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_audio_test");
    let compile = support::run(
        support::environment()
            .clang_command()
            .current_dir(root)
            .args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Wpedantic",
                "-Werror",
                "-Iruntime/crumb",
                "tests/crumb_audio.c",
                "runtime/crumb/audio.c",
                "-pthread",
                "-lm",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("audio mixer test compilation", &compile);
    assert_success(
        "audio mixer test",
        &support::run(&mut Command::new(executable)),
    );
}

#[test]
fn audio_builtins_are_typed_void_calls_and_headless_playback_is_silent() {
    let directory = support::workspace();
    let source = directory.path().join("audio.spk");
    let program = r#"game "Audio"
        start {
            tone(880.0, 0.08, 0.5)
            noise(0.05, 0.2)
            print_i32(42)
        }
        update(dt: f32) {}
        draw {}
    "#;
    fs::write(&source, program).unwrap();
    let executable = support::build_in(directory.path(), &source);
    let run = support::run(Command::new(executable).current_dir(directory.path()));
    assert_success("headless audio", &run);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");
    assert!(run.stderr.is_empty());
    let ir = fs::read_to_string(directory.path().join("build/audio.ll")).unwrap();
    assert!(ir.contains("declare void @crumb_tone(float, float, float)"));
    assert!(ir.contains("call void @crumb_tone("));
    assert!(ir.contains("call void @crumb_noise("));
    for invalid in [
        "tone(880, 0.08, 0.5)",
        "noise(0.05)",
        "let x: f32 = noise(0.1, 1.0)",
    ] {
        assert!(speck::analyze(&program.replace("tone(880.0, 0.08, 0.5)", invalid)).is_err());
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn native_audio_cleans_up_partial_initialization_and_active_callbacks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_audio_macos_test");
    let compile = support::run(
        support::environment()
            .clang_command()
            .current_dir(root)
            .args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Wpedantic",
                "-Werror",
                "-Iruntime/crumb",
                "tests/crumb_audio_macos.c",
                "runtime/crumb/audio.c",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("audio lifecycle test compilation", &compile);
    let run = support::run(&mut Command::new(executable));
    assert_success("audio lifecycle test", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        "CRuMB could not initialize audio; continuing silently\n".repeat(8)
    );
}
