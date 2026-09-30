pub mod support;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;
use support::assert_success;

fn compile_harness(directory: &Path) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = directory.join("crumb_storage_test");
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
                "tests/crumb_storage.c",
                "runtime/crumb/storage.c",
                "-o",
            ])
            .arg(&executable),
    );
    assert_success("storage test compilation", &compile);
    executable
}

fn run(executable: &Path, save_root: &Path, arguments: &[&str]) -> Output {
    support::run(
        Command::new(executable)
            .current_dir(
                executable
                    .parent()
                    .expect("test executable has a directory"),
            )
            .env("SPECK_SAVE_DIR", save_root)
            .args(arguments),
    )
}

fn default_root_command(
    executable: &Path,
    home: &Path,
    xdg_data_home: Option<&Path>,
    arguments: &[&str],
) -> Command {
    let mut command = Command::new(executable);
    command
        .current_dir(
            executable
                .parent()
                .expect("test executable has a directory"),
        )
        .env_remove("SPECK_SAVE_DIR")
        .env("HOME", home)
        .env_remove("XDG_DATA_HOME")
        .args(arguments);
    if let Some(xdg_data_home) = xdg_data_home {
        command.env("XDG_DATA_HOME", xdg_data_home);
    }
    command
}

fn game_directories(save_root: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(save_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
fn storage_is_portable_bounded_namespaced_and_failure_safe() {
    let directory = support::workspace();
    let test_root = directory.path();
    let executable = compile_harness(test_root);
    let save_root = test_root.join("save-root");
    let first_identity = "First Game/../../must-not-be-a-path";
    let second_identity = "Second Game";

    assert_success(
        "write persistent integer fixture",
        &run(&executable, &save_root, &["write-fixture", first_identity]),
    );
    assert_success(
        "read persistent integer fixture after restart",
        &run(&executable, &save_root, &["read-fixture", first_identity]),
    );

    let first_directories = game_directories(&save_root);
    assert_eq!(first_directories.len(), 1);
    let game_directory = &first_directories[0];
    assert!(
        game_directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("game-")
    );
    assert_eq!(
        fs::metadata(game_directory).unwrap().permissions().mode() & 0o077,
        0,
        "game storage should not grant group or other permissions"
    );
    assert_eq!(
        fs::metadata(game_directory.join("slot-00.i32"))
            .unwrap()
            .permissions()
            .mode()
            & 0o077,
        0,
        "slot storage should not grant group or other permissions"
    );

    for malformed in [
        &b""[..],
        &b"SPECK-I32-V2\n7\n"[..],
        &b"SPECK-I32-V1\n2147483648\n"[..],
        &b"SPECK-I32-V1\n01\n"[..],
        &b"SPECK-I32-V1\n7\ntrailing"[..],
    ] {
        fs::write(game_directory.join("slot-02.i32"), malformed).unwrap();
        assert_success(
            "malformed slot falls back",
            &run(
                &executable,
                &save_root,
                &["get", first_identity, "2", "765", "765"],
            ),
        );
    }
    fs::write(game_directory.join("slot-02.i32"), vec![b'7'; 4096]).unwrap();
    assert_success(
        "oversized slot falls back without an unbounded read",
        &run(
            &executable,
            &save_root,
            &["get", first_identity, "2", "765", "765"],
        ),
    );
    assert_success(
        "restore malformed slot",
        &run(
            &executable,
            &save_root,
            &["put", first_identity, "2", "-123456789"],
        ),
    );

    let canary = test_root.join("canary");
    fs::write(&canary, b"must remain unchanged").unwrap();
    symlink(&canary, game_directory.join("slot-06.i32")).unwrap();
    assert_success(
        "symlinked slot is not followed while loading",
        &run(
            &executable,
            &save_root,
            &["get", first_identity, "6", "444", "444"],
        ),
    );
    assert_success(
        "save atomically replaces a symlink entry",
        &run(
            &executable,
            &save_root,
            &["put", first_identity, "6", "606"],
        ),
    );
    assert_eq!(fs::read(&canary).unwrap(), b"must remain unchanged");

    let fifo = game_directory.join("slot-07.i32");
    assert_success(
        "create unsupported FIFO slot",
        &support::run(Command::new("mkfifo").arg(&fifo)),
    );
    let fifo_load = support::run_with_timeout(
        Command::new(&executable)
            .env("SPECK_SAVE_DIR", &save_root)
            .args(["get", first_identity, "7", "707", "707"]),
        Duration::from_secs(1),
    );
    assert_success("FIFO slot returns fallback without blocking", &fifo_load);

    let mut left = Command::new(&executable)
        .env("SPECK_SAVE_DIR", &save_root)
        .args(["put", first_identity, "4", "404"])
        .spawn()
        .unwrap();
    let mut right = Command::new(&executable)
        .env("SPECK_SAVE_DIR", &save_root)
        .args(["put", first_identity, "5", "505"])
        .spawn()
        .unwrap();
    assert!(left.wait().unwrap().success());
    assert!(right.wait().unwrap().success());
    for (slot, expected) in [(0, i32::MIN), (1, i32::MAX), (4, 404), (5, 505), (6, 606)] {
        assert_success(
            "independent slot survives atomic writes",
            &run(
                &executable,
                &save_root,
                &[
                    "get",
                    first_identity,
                    &slot.to_string(),
                    &expected.to_string(),
                    "999",
                ],
            ),
        );
    }
    assert!(fs::read_dir(game_directory).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")
    }));

    assert_success(
        "missing namespace uses fallback",
        &run(
            &executable,
            &save_root,
            &["get", second_identity, "0", "808", "808"],
        ),
    );
    assert_eq!(
        game_directories(&save_root).len(),
        1,
        "loads must not create storage"
    );
    assert_success(
        "write isolated namespace",
        &run(
            &executable,
            &save_root,
            &["put", second_identity, "0", "212"],
        ),
    );
    assert_eq!(game_directories(&save_root).len(), 2);
    assert_success(
        "first namespace remains isolated",
        &run(
            &executable,
            &save_root,
            &["get", first_identity, "0", &i32::MIN.to_string(), "0"],
        ),
    );

    let blocked_root = test_root.join("blocked-root");
    fs::write(&blocked_root, b"a file cannot contain game storage").unwrap();
    assert_success(
        "write failure is graceful",
        &run(
            &executable,
            &blocked_root,
            &["expect-failure", first_identity],
        ),
    );
    assert_success(
        "relative override is rejected",
        &run(
            &executable,
            Path::new("relative-save-root"),
            &["expect-failure", first_identity],
        ),
    );
}

#[test]
fn storage_uses_the_platform_default_root_without_touching_the_real_home() {
    let directory = support::workspace();
    let test_root = directory.path();
    let executable = compile_harness(test_root);
    let home = test_root.join("isolated-home");
    let xdg_data_home = test_root.join("isolated-xdg-data");
    let identity = "Default Root Game";

    #[cfg(target_os = "macos")]
    {
        let write = support::run(&mut default_root_command(
            &executable,
            &home,
            Some(&xdg_data_home),
            &["put", identity, "0", "314"],
        ));
        assert_success("write under macOS default root", &write);
        let application_support = home.join("Library/Application Support/Speck");
        assert_eq!(game_directories(&application_support).len(), 1);
        assert!(
            !xdg_data_home.exists(),
            "macOS storage should ignore XDG_DATA_HOME"
        );
    }

    #[cfg(target_os = "linux")]
    {
        let write = support::run(&mut default_root_command(
            &executable,
            &home,
            Some(&xdg_data_home),
            &["put", identity, "0", "314"],
        ));
        assert_success("write under XDG data root", &write);
        assert_eq!(game_directories(&xdg_data_home.join("speck")).len(), 1);
        assert!(
            !home.exists(),
            "an absolute XDG_DATA_HOME should take precedence over HOME"
        );

        let relative_xdg = Path::new("relative-xdg-data");
        let fallback_write = support::run(&mut default_root_command(
            &executable,
            &home,
            Some(relative_xdg),
            &["put", "Home Fallback Game", "0", "271"],
        ));
        assert_success("fall back from relative XDG data root", &fallback_write);
        assert_eq!(game_directories(&home.join(".local/share/speck")).len(), 1);
    }

    let read = support::run(&mut default_root_command(
        &executable,
        &home,
        Some(&xdg_data_home),
        &["get", identity, "0", "314", "0"],
    ));
    assert_success("read from platform default root after restart", &read);
}
