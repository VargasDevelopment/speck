pub mod support;

use std::fs;
use std::process::Command;
use support::assert_success;

#[test]
fn empty_and_embedded_nul_titles_have_complete_distinct_identities() {
    let directory = support::workspace();
    let work = directory.path();
    let source = work.join("identity.spk");
    let saves = work.join("saves");
    for title in ["", "A", "A\0B", "A\0C"] {
        fs::write(
            &source,
            format!(
                "game \"{title}\" start {{
                    print_i32(load_i32(0, -1))
                    if save_i32(0, 42) {{ print_i32(1) }} else {{ print_i32(0) }}
                    quit()
                }} update(dt: f32) {{}} draw {{}}"
            ),
        )
        .unwrap();
        let executable = support::build_in(work, &source);
        for expected in [b"-1\n1\n".as_slice(), b"42\n1\n"] {
            let output = support::run(
                Command::new(&executable)
                    .current_dir(work)
                    .env("SPECK_SAVE_DIR", &saves),
            );
            assert_success("complete title namespace", &output);
            assert_eq!(output.stdout, expected, "title {title:?}");
        }
    }
    assert_eq!(fs::read_dir(&saves).unwrap().count(), 4);
}

#[test]
fn compiled_game_persists_values_across_relocation_and_process_restart() {
    let directory = support::workspace();
    let work = directory.path();
    let saves = work.join("saves");
    let source = work.join("score.spk");
    let program = r#"game "Score — café"
        start {
            print_i32(load_i32(0, -7))
            if save_i32(0, 2147483647) { print_i32(1) } else { print_i32(0) }
            if save_i32(16, 123) { print_i32(0) } else { print_i32(1) }
            print_i32(load_i32(-1, 19))
            quit()
        }
        update(dt: f32) {} draw {}
    "#;
    fs::write(&source, program).unwrap();
    let executable = support::build_in(work, &source);
    let run = |executable: &std::path::Path, cwd: &std::path::Path| {
        let output = support::run(
            Command::new(executable)
                .current_dir(cwd)
                .env("SPECK_SAVE_DIR", &saves),
        );
        assert_success("compiled persistent score", &output);
        output.stdout
    };
    assert_eq!(run(&executable, work), b"-7\n1\n1\n19\n");

    let elsewhere = work.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let relocated = elsewhere.join("renamed-game");
    fs::rename(&executable, &relocated).unwrap();
    fs::remove_file(&source).unwrap();
    assert_eq!(run(&relocated, &elsewhere), b"2147483647\n1\n1\n19\n");

    // A different title has an independent namespace, even for the same source path.
    fs::write(&source, program.replace("Score — café", "Another game")).unwrap();
    let other_game = support::build_in(work, &source);
    assert_eq!(run(&other_game, work), b"-7\n1\n1\n19\n");

    for invalid in ["load_i32(0.0, -7)", "load_i32(0)", "save_i32(0, true)"] {
        assert!(speck::analyze(&program.replace("load_i32(0, -7)", invalid)).is_err());
    }
}
