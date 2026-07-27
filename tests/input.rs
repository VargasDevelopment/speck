use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[test]
fn portable_input_state_has_deterministic_transitions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = root.join("target/crumb_input_test");
    let compile = Command::new("clang")
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
        .arg(&executable)
        .output()
        .expect("Clang should start");
    assert_success("input-state test compilation", &compile);

    let run = Command::new(&executable)
        .output()
        .expect("input-state test should start");
    assert_success("input-state test", &run);
}

#[test]
fn headless_keyboard_example_is_input_free_and_deterministic() {
    const WIDTH: usize = 320;
    const CHANNELS: usize = 3;
    const HEADER: &[u8] = b"P6\n320 180\n255\n";

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let work = root.join("target/input_example_e2e");
    fs::create_dir_all(&work).expect("test working directory should exist");
    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(&work)
        .arg("build")
        .arg(root.join("examples/keyboard_rectangle.spk"))
        .output()
        .expect("Speck compiler should start");
    assert_success("keyboard example build", &build);

    let run = Command::new(work.join("build/keyboard_rectangle"))
        .current_dir(&work)
        .output()
        .expect("keyboard example should start");
    assert_success("headless keyboard example", &run);
    let ppm = fs::read(work.join("build/frame.ppm")).expect("frame.ppm should exist");
    let pixel = |x: usize, y: usize| {
        let offset = HEADER.len() + (y * WIDTH + x) * CHANNELS;
        &ppm[offset..offset + CHANNELS]
    };
    assert_eq!(pixel(150, 80), [240, 150, 40]);
    assert_eq!(pixel(149, 80), [255, 255, 255]);
}

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
