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

#[test]
fn all_browser_key_transitions_reach_the_c_stream_runtime() {
    use speck::dev::protocol::{BrowserInput, ControlMessage, encode_control, parse_browser_input};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = support::workspace();
    let executable = directory.path().join("crumb_stream_input_test");
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
                "tests/crumb_stream_input.c",
                "runtime/crumb/input.c",
                "runtime/crumb/framebuffer.c",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("stream input test compilation", &compile);

    let mut records = Vec::new();
    let mut send = |body: String| {
        let BrowserInput::Key { key, down, .. } = parse_browser_input(body.as_bytes()).unwrap()
        else {
            panic!("supported key should parse");
        };
        records.extend(encode_control(ControlMessage::Key { key, down }));
    };
    for key in speck::keyboard::KEYS {
        for transition in ["down", "down", "up"] {
            send(format!("viewer {transition} {}", key.browser_code));
        }
    }
    for key in speck::keyboard::KEYS {
        send(format!("viewer down {}", key.browser_code));
    }
    records.extend(encode_control(ControlMessage::ReleaseAll));
    // Invalid wire IDs must stay harmless at the C runtime boundary.
    records.extend([b'S', b'P', b'K', b'I', 1, 1, 255, 1]);
    let input = directory.path().join("input.bin");
    fs::write(&input, records).unwrap();
    let run = support::run(Command::new(&executable).arg(input));
    assert_success("stream input test", &run);
}

#[test]
fn keyboard_catalog_is_available_to_games_with_stable_legacy_ids() {
    use speck::keyboard::{KEYS, Key};
    for (name, id) in [
        ("KEY_W", 0),
        ("KEY_A", 1),
        ("KEY_S", 2),
        ("KEY_D", 3),
        ("KEY_UP", 4),
        ("KEY_DOWN", 5),
        ("KEY_LEFT", 6),
        ("KEY_RIGHT", 7),
        ("KEY_SPACE", 8),
        ("KEY_ENTER", 9),
        ("KEY_ESCAPE", 10),
        ("KEY_F", 11),
    ] {
        assert_eq!(
            KEYS.iter().find(|key| key.name == name).unwrap().key as u8,
            id
        );
    }
    for letter in 'A'..='Z' {
        assert!(KEYS.iter().any(|key| key.name == format!("KEY_{letter}")));
    }
    for digit in 0..=9 {
        for name in [format!("KEY_{digit}"), format!("KEY_NUMPAD_{digit}")] {
            assert!(KEYS.iter().any(|key| key.name == name));
        }
    }
    for function in 1..=24 {
        assert!(
            KEYS.iter()
                .any(|key| key.name == format!("KEY_F{function}"))
        );
    }
    // Compile every predefined name and inspect the actual emitted ABI value.
    // This connects catalog coverage to semantic lookup and LLVM lowering.
    for key in KEYS {
        let source = format!(
            "game \"Keyboard\" start {{ print_i32({}) }} update(dt: f32) {{}} draw {{}}",
            key.name
        );
        let ir = speck::compile_to_llvm(&source).unwrap();
        assert!(ir.contains(&format!(
            "call void @crumb_print_i32(i32 {})",
            key.key as u8
        )));
        assert_eq!(Key::from_browser_code(key.browser_code), Some(key.key));
        assert_eq!(Key::from_id(key.key as u8), Some(key.key));
    }
    assert!(Key::from_id(255).is_none());
    assert!(
        speck::analyze(
            "game \"Invalid\" start { print_i32(KEY_NOT_REAL) } update(dt: f32) {} draw {}"
        )
        .is_err()
    );
}
