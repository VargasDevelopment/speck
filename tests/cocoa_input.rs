#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::Path;
use std::process::{Command, Output};

#[test]
fn cocoa_translates_keys_repeats_escape_and_focus_loss() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = root.join("target/crumb_cocoa_input_test");
    let compile = Command::new("xcrun")
        .current_dir(root)
        .args([
            "clang",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-Werror",
            "-DCRUMB_COCOA=1",
            "-DCRUMB_PACED=1",
            "-Iruntime/crumb",
            "-x",
            "objective-c",
            "tests/crumb_cocoa_input.m",
            "-x",
            "c",
            "runtime/crumb/input.c",
            "runtime/crumb/framebuffer.c",
            "-framework",
            "AppKit",
            "-framework",
            "CoreGraphics",
            "-o",
        ])
        .arg(&executable)
        .output()
        .expect("Apple Clang should start");
    assert_success("Cocoa input test compilation", &compile);

    let run = Command::new(&executable)
        .output()
        .expect("Cocoa input test should start");
    assert_success("Cocoa input test", &run);
}

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
