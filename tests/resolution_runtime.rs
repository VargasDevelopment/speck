pub mod support;

use std::{fs, process::Command};

#[test]
fn configured_dimensions_reach_ppm_and_native_stream_presenters() {
    let directory = support::workspace();
    let work = directory.path();
    fs::write(
        work.join("sized.spk"),
        r#"
        game "Sized" resolution(333, 197)
        start { print_i32(FRAMEBUFFER_WIDTH) print_i32(FRAMEBUFFER_HEIGHT) }
        update(dt: f32) {}
        draw {
            clear_rgb(19, 27, 41)
            fill_rect(FRAMEBUFFER_WIDTH - 1, FRAMEBUFFER_HEIGHT - 1, 10, 10, 240, 230, 220)
        }
    "#,
    )
    .unwrap();
    let compiler = env!("CARGO_BIN_EXE_speck");
    let built = support::run(
        Command::new(compiler)
            .current_dir(work)
            .args(["build", "sized.spk"]),
    );
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let output = support::run(Command::new(work.join("build/sized")).current_dir(work));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"333\n197\n");
    let mut expected = b"P6\n333 197\n255\n".to_vec();
    expected.extend([19_u8, 27, 41].repeat(333 * 197 - 1));
    expected.extend([240, 230, 220]);
    assert_eq!(fs::read(work.join("build/frame.ppm")).unwrap(), expected);

    // Real C frame encoding must agree with the Rust receiver, not just roundtrip itself.
    let streamed = support::run(Command::new(compiler).current_dir(work).args([
        "dev",
        "sized.spk",
        "--port",
        "0",
        "--frames",
        "3",
    ]));
    assert!(
        streamed.status.success(),
        "{}",
        String::from_utf8_lossy(&streamed.stderr)
    );
    assert!(String::from_utf8_lossy(&streamed.stdout).contains("Frames received: 3"));
}
