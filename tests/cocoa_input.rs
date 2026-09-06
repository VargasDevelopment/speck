#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

pub mod support;

use std::path::Path;
use std::process::Command;
use support::assert_success;

#[test]
fn cocoa_translates_keys_repeats_escape_and_focus_loss() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_cocoa_input_test");
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
            .arg(&executable),
    );
    assert_success("Cocoa input test compilation", &compile);

    let run = support::run(&mut Command::new(&executable));
    assert_success("Cocoa input test", &run);
}
