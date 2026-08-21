use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const WIDTH: usize = 320;
const HEIGHT: usize = 180;
const CHANNELS: usize = 3;
const PPM_HEADER: &[u8] = b"P6\n320 180\n255\n";

#[test]
fn framebuffer_clips_rectangles_and_clamps_colors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = root.join("target/crumb_framebuffer_test");
    let compile = Command::new("clang")
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
        .arg(&executable)
        .output()
        .expect("Clang should start");
    assert_success("framebuffer test compilation", &compile);

    let run = Command::new(&executable)
        .output()
        .expect("framebuffer test should start");
    assert_success("framebuffer test", &run);
}

#[test]
fn speck_program_writes_expected_ppm_framebuffer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_root = root.join("target/framebuffer_e2e");
    fs::create_dir_all(&test_root).expect("test working directory should be created");

    let build = Command::new(env!("CARGO_BIN_EXE_speck"))
        .current_dir(&test_root)
        .args(["build"])
        .arg(root.join("examples/framebuffer_rect.spk"))
        .output()
        .expect("Speck compiler should start");
    assert_success("Speck framebuffer example build", &build);

    let run = Command::new(test_root.join("build/framebuffer_rect"))
        .current_dir(&test_root)
        .output()
        .expect("compiled framebuffer example should start");
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

fn assert_success(description: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{description} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
