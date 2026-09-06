pub mod support;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::{fs::PermissionsExt, process::CommandExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct Watcher {
    child: Child,
    directory: tempfile::TempDir,
    address: SocketAddr,
}

impl Watcher {
    fn start(
        directory: tempfile::TempDir,
        frames: Option<&str>,
        extra_path: Option<&Path>,
    ) -> Self {
        let work = directory.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_speck"));
        command
            .current_dir(work)
            .args(["dev", "game.spk", "--watch", "--port", "0"])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(File::create(work.join("stdout")).unwrap())
            .stderr(File::create(work.join("stderr")).unwrap());
        if let Some(frames) = frames {
            command.args(["--frames", frames]);
        }
        if let Some(path) = extra_path {
            let inherited_path = std::env::var_os("PATH").unwrap();
            let paths =
                std::iter::once(path.to_owned()).chain(std::env::split_paths(&inherited_path));
            command.env("PATH", std::env::join_paths(paths).unwrap());
        }
        let child = command.spawn().unwrap();
        let mut watcher = Self {
            child,
            directory,
            address: "127.0.0.1:1".parse().unwrap(),
        };
        watcher.until(|this| this.stdout().contains("Viewer URL: "));
        watcher.address = watcher
            .stdout()
            .lines()
            .find_map(|line| line.strip_prefix("Viewer URL: http://"))
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        watcher
    }

    fn work(&self) -> &Path {
        self.directory.path()
    }
    fn stdout(&self) -> String {
        fs::read_to_string(self.work().join("stdout")).unwrap()
    }
    fn stderr(&self) -> String {
        fs::read_to_string(self.work().join("stderr")).unwrap()
    }
    fn write(&self, file: &str, source: &str) {
        fs::write(self.work().join(file), source).unwrap();
    }
    fn until(&mut self, mut condition: impl FnMut(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition(self) {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "watcher exited\n{}\n{}",
                self.stdout(),
                self.stderr()
            );
            assert!(
                Instant::now() < deadline,
                "watcher timed out\n{}\n{}",
                self.stdout(),
                self.stderr()
            );
            thread::sleep(Duration::from_millis(30));
        }
    }
    fn request(&self, method: &str, path: &str, body: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(2)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        bytes
    }
    fn frame(&self, after: u64) -> Response {
        Response::parse(self.request("GET", &format!("/frame?after={after}"), ""))
    }
    fn color(&mut self, red: u8, after: u64) -> Response {
        let mut found = None;
        self.until(|this| {
            let response = this.frame(after);
            if response.body.first() == Some(&red) && response.body.len() == 320 * 180 * 3 {
                found = Some(response);
                true
            } else {
                false
            }
        });
        found.unwrap()
    }
    fn game_pid(&self) -> i32 {
        self.stdout()
            .lines()
            .filter_map(|line| {
                line.rsplit_once("(pid ")
                    .map(|(_, pid)| pid.trim_end_matches(')').parse().unwrap())
            })
            .next_back()
            .unwrap()
    }
    fn stop(&mut self) {
        signal(self.child.id() as i32, 2);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "watch stop failed\n{}\n{}",
                    self.stdout(),
                    self.stderr()
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher ignored SIGINT\n{}",
                self.stderr()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        signal(self.child.id() as i32, 2);
        let deadline = Instant::now() + Duration::from_secs(4);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        // Failure cleanup also covers the deliberately blocked compiler's independent group.
        if let Ok(pid) = fs::read_to_string(self.work().join("compiler.pid"))
            && let Ok(pid) = pid.trim().parse::<i32>()
        {
            signal(-pid, 9);
        }
        signal(-(self.child.id() as i32), 9);
        let _ = self.child.wait();
    }
}

struct Response {
    headers: String,
    body: Vec<u8>,
}
impl Response {
    fn parse(bytes: Vec<u8>) -> Self {
        let split = bytes
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap()
            + 4;
        Self {
            headers: String::from_utf8(bytes[..split].to_vec()).unwrap(),
            body: bytes[split..].to_vec(),
        }
    }
    fn header(&self, name: &str) -> &str {
        self.headers
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap()
            .trim()
    }
    fn sequence(&self) -> u64 {
        self.header("X-Speck-Sequence:").parse().unwrap()
    }
    fn generation(&self) -> u64 {
        self.header("X-Speck-Generation:").parse().unwrap()
    }
}

fn signal(pid: i32, value: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: tests signal only their owned watcher/compiler groups and recorded game PIDs.
    unsafe {
        kill(pid, value);
    }
}
fn alive(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: signal zero inspects process existence without changing it.
    unsafe { kill(pid, 0) == 0 }
}
fn game(red: &str) -> String {
    format!("game \"Watch\"\nstart {{}}\nupdate(dt: f32) {{}}\ndraw {{ clear_rgb({red}, 0, 0) }}\n")
}
fn imported_game() -> String {
    game("palette::red()").replacen('\n', "\nimport \"palette.spk\" as palette\n", 1)
}

fn palette(red: i32) -> String {
    format!("fn red() -> i32 {{ return {red} }}\n")
}

#[test]
fn dependency_edits_errors_and_missing_files_recover_at_the_same_viewer() {
    let directory = support::workspace();
    fs::write(directory.path().join("game.spk"), imported_game()).unwrap();
    fs::write(directory.path().join("palette.spk"), palette(11)).unwrap();
    let mut watch = Watcher::start(directory, None, None);
    let first = watch.color(11, 0);
    let first_pid = watch.game_pid();
    watch.write("palette.spk", &palette(22));
    let second = watch.color(22, first.sequence());
    assert!(second.sequence() > first.sequence());
    assert!(second.generation() > first.generation());
    assert!(!alive(first_pid));

    let second_pid = watch.game_pid();
    watch.write("palette.spk", "fn red( broken");
    watch.until(|this| this.frame(second.sequence()).header("X-Speck-State:") == "error");
    assert!(!alive(second_pid));
    assert!(
        watch
            .request(
                "POST",
                &format!("/input?generation={}", second.generation()),
                "test down KeyA"
            )
            .starts_with(b"HTTP/1.1 503")
    );

    watch.write(
        "palette.spk",
        "import \"missing.spk\" as color\nfn red() -> i32 { return color::red() }\n",
    );
    watch.until(|this| this.stderr().contains("missing.spk"));
    watch.write("missing.spk", &palette(33));
    let third = watch.color(33, second.sequence());
    fs::remove_file(watch.work().join("palette.spk")).unwrap();
    watch.until(|this| this.frame(third.sequence()).header("X-Speck-State:") == "error");
    watch.write("palette.spk", &palette(44));
    let fourth = watch.color(44, third.sequence());

    watch.write(
        "palette.spk",
        "fn red() -> i32 {\n    let zero: i32 = 0\n    return 1 / zero\n}\n",
    );
    watch.until(|this| {
        this.stderr().contains("palette.spk:3:12:") && this.stderr().contains("division")
    });
    watch.until(|this| this.frame(u64::MAX).header("X-Speck-State:") == "watching");
    assert!(!alive(watch.game_pid()));

    watch.write("game.spk", &game("55"));
    let fifth = watch.color(55, fourth.sequence());
    watch.write("palette.spk", "this is no longer imported");
    thread::sleep(Duration::from_millis(700));
    assert_eq!(
        watch.frame(fifth.sequence()).generation(),
        fifth.generation()
    );
    let final_pid = watch.game_pid();
    // A real browser may have input in flight when the process is interrupted.
    assert!(
        watch
            .request(
                "POST",
                &format!("/input?generation={}", fifth.generation()),
                "test down KeyA"
            )
            .starts_with(b"HTTP/1.1 204")
    );
    watch.stop();
    assert!(!alive(final_pid));
}

#[test]
fn missing_initial_source_and_finite_games_keep_watching_until_interrupt() {
    let mut watch = Watcher::start(support::workspace(), Some("1"), None);
    watch.until(|this| this.frame(0).header("X-Speck-State:") == "error");
    watch.write("game.spk", &game("66"));
    watch.until(|this| this.stdout().contains("Development game:"));
    watch.until(|this| this.frame(u64::MAX).header("X-Speck-State:") == "watching");
    assert!(!alive(watch.game_pid()));
    watch.write("game.spk", "broken");
    watch.until(|this| this.frame(u64::MAX).header("X-Speck-State:") == "error");
    watch.stop();
}

#[test]
fn atomic_saves_and_symlink_retargets_follow_the_new_source_graph() {
    use std::os::unix::fs::symlink;
    let directory = support::workspace();
    let work = directory.path();
    for (folder, color) in [("a", 21), ("b", 31)] {
        fs::create_dir(work.join(folder)).unwrap();
        fs::write(work.join(folder).join("game.spk"), imported_game()).unwrap();
        fs::write(work.join(folder).join("palette.spk"), palette(color)).unwrap();
    }
    symlink("a/game.spk", work.join("game.spk")).unwrap();
    let mut watch = Watcher::start(directory, None, None);
    let first = watch.color(21, 0);
    symlink("b/game.spk", watch.work().join("saved.spk")).unwrap();
    fs::rename(
        watch.work().join("saved.spk"),
        watch.work().join("game.spk"),
    )
    .unwrap();
    let second = watch.color(31, first.sequence());
    assert!(second.generation() > first.generation());
    watch.write("b/saved.spk", &palette(41));
    fs::rename(
        watch.work().join("b/saved.spk"),
        watch.work().join("b/palette.spk"),
    )
    .unwrap();
    let third = watch.color(41, second.sequence());
    watch.write("a/palette.spk", &palette(51));
    thread::sleep(Duration::from_millis(700));
    assert_eq!(
        watch.frame(third.sequence()).generation(),
        third.generation()
    );
    watch.stop();
}

fn script(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[test]
fn edits_during_native_build_are_not_lost_and_sigint_reaps_build_descendants() {
    let directory = support::workspace();
    let work = directory.path();
    let bin = work.join("bin");
    fs::create_dir(&bin).unwrap();
    let clang = support::environment().clang();
    script(
        &bin.join("clang"),
        &format!(
            r#"#!/bin/sh
if [ "$1" != '-dumpmachine' ] && [ -f block ]; then
  echo $$ > compiler.pid
  sleep 60 &
  echo $! > descendant.pid
  while [ -f block ]; do sleep 0.05; done
  kill "$(cat descendant.pid)"
  wait
fi
exec {} "$@"
"#,
            quote(clang)
        ),
    );
    // macOS discovers Clang via xcrun first; preserve the other real discovery responses.
    script(
        &bin.join("xcrun"),
        &format!(
            r#"#!/bin/sh
if [ "$1" = '--find' ] && [ "$2" = 'clang' ]; then
  printf '%s\n' {}
else
  exec /usr/bin/xcrun "$@"
fi
"#,
            quote(&bin.join("clang"))
        ),
    );
    fs::write(work.join("game.spk"), game("10")).unwrap();
    fs::write(work.join("block"), "").unwrap();
    let mut watch = Watcher::start(directory, None, Some(&bin));
    watch.until(|this| this.work().join("compiler.pid").is_file());
    watch.write("game.spk", &game("77"));
    fs::remove_file(watch.work().join("block")).unwrap();
    let first = watch.color(77, 0);
    assert_eq!(
        first.generation(),
        1,
        "the outdated build must never launch"
    );
    assert_eq!(watch.stdout().matches("Development game:").count(), 1);

    fs::remove_file(watch.work().join("compiler.pid")).unwrap();
    fs::remove_file(watch.work().join("descendant.pid")).unwrap();
    watch.write("block", "");
    watch.write("game.spk", &game("88"));
    watch.until(|this| {
        ["compiler.pid", "descendant.pid"].iter().all(|name| {
            fs::read_to_string(this.work().join(name))
                .ok()
                .and_then(|text| text.trim().parse::<i32>().ok())
                .is_some()
        })
    });
    let compiler: i32 = fs::read_to_string(watch.work().join("compiler.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let descendant: i32 = fs::read_to_string(watch.work().join("descendant.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(compiler));
    watch.stop();
    assert!(!alive(compiler));
    // On Linux an adopted zombie may briefly remain until init reaps it.
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive(descendant) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !alive(descendant),
        "compiler descendant survived cancellation"
    );
}
