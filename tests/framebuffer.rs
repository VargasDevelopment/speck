pub mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use support::assert_success;

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const CHANNELS: usize = 3;
const PPM_HEADER: &[u8] = b"P6\n320 180\n255\n";

#[test]
fn framebuffer_clips_rectangles_and_clamps_colors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_framebuffer_test");
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
                "tests/crumb_framebuffer.c",
                "runtime/crumb/framebuffer.c",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("framebuffer test compilation", &compile);

    let run = support::run(&mut Command::new(&executable));
    assert_success("framebuffer test", &run);
}

#[test]
fn speck_program_writes_expected_ppm_framebuffer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let test_root = directory.path();

    let build = support::run(
        Command::new(env!("CARGO_BIN_EXE_speck"))
            .current_dir(test_root)
            .args(["build"])
            .arg(root.join("examples/framebuffer_rect.spk")),
    );
    assert_success("Speck framebuffer example build", &build);

    let run =
        support::run(Command::new(test_root.join("build/framebuffer_rect")).current_dir(test_root));
    assert_success("compiled framebuffer example", &run);

    let ppm = fs::read(test_root.join("build/frame.ppm")).expect("frame.ppm should be written");
    let mut expected_pixels = vec![0_u8; WIDTH * HEIGHT * CHANNELS];
    for pixel in expected_pixels.as_chunks_mut::<CHANNELS>().0 {
        pixel.copy_from_slice(&[10, 20, 30]);
    }
    for y in 8..18 {
        for x in 12..32 {
            let offset = (y * WIDTH + x) * CHANNELS;
            expected_pixels[offset..offset + CHANNELS].copy_from_slice(&[240, 120, 30]);
        }
    }

    assert_eq!(ppm.len(), PPM_HEADER.len() + expected_pixels.len());
    assert_eq!(&ppm[..PPM_HEADER.len()], PPM_HEADER);
    assert_eq!(&ppm[PPM_HEADER.len()..], expected_pixels);
}
