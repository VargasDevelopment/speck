pub mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::assert_success;

#[test]
fn portable_input_state_has_deterministic_transitions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_input_test");
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
                "tests/crumb_input.c",
                "runtime/crumb/input.c",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("input-state test compilation", &compile);

    let run = support::run(&mut Command::new(&executable));
    assert_success("input-state test", &run);
}

#[test]
fn headless_keyboard_example_is_input_free_and_deterministic() {
    const WIDTH: usize = 320;
    const CHANNELS: usize = 3;
    const HEADER: &[u8] = b"P6\n320 180\n255\n";

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let work = directory.path();
    let build = support::run(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(work)
            .arg("build")
            .arg(root.join("examples/keyboard_rectangle.spk")),
    );
    assert_success("keyboard example build", &build);

    let run = support::run(Command::new(work.join("build/keyboard_rectangle")).current_dir(work));
    assert_success("headless keyboard example", &run);
    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame.ppm should exist");
    let pixel = |x: usize, y: usize| {
        let offset = HEADER.len() + (y * WIDTH + x) * CHANNELS;
        &ppm[offset..offset + CHANNELS]
    };
    assert_eq!(pixel(150, 80), [240, 150, 40]);
    assert_eq!(pixel(149, 80), [32, 18, 32]);
}
