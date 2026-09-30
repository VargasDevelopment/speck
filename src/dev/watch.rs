//! Poll the analyzed dependency closure and restart whole games on stable edits.
use std::collections::BTreeMap;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use super::{Options, server::ViewerState, session::Session};
use crate::source::SourceMap;
use crate::{analyze_path, codegen, toolchain};

const POLL: Duration = Duration::from_millis(100);
const QUIET: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    target: Option<PathBuf>,
    observation: Observation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Observation {
    Contents(Result<Vec<u8>, io::ErrorKind>),
    Metadata(Result<MetadataStamp, io::ErrorKind>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MetadataStamp {
    length: u64,
    modified: Result<SystemTime, io::ErrorKind>,
    is_file: bool,
    is_directory: bool,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
}

impl Stamp {
    fn contents(path: &Path) -> Self {
        Self {
            target: path.canonicalize().ok(),
            observation: Observation::Contents(fs::read(path).map_err(|error| error.kind())),
        }
    }

    fn metadata(path: &Path) -> Self {
        Self {
            target: path.canonicalize().ok(),
            observation: Observation::Metadata(
                fs::metadata(path)
                    .map(MetadataStamp::from)
                    .map_err(|error| error.kind()),
            ),
        }
    }

    fn refresh(&self, path: &Path) -> Self {
        match self.observation {
            Observation::Contents(_) => Self::contents(path),
            Observation::Metadata(_) => Self::metadata(path),
        }
    }

    fn observed_contents(&self) -> Option<&Result<Vec<u8>, io::ErrorKind>> {
        match &self.observation {
            Observation::Contents(contents) => Some(contents),
            Observation::Metadata(_) => None,
        }
    }
}

impl From<fs::Metadata> for MetadataStamp {
    fn from(metadata: fs::Metadata) -> Self {
        Self {
            length: metadata.len(),
            modified: metadata.modified().map_err(|error| error.kind()),
            is_file: metadata.is_file(),
            is_directory: metadata.is_dir(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
}

type Snapshot = BTreeMap<PathBuf, Stamp>;

fn current(previous: &Snapshot) -> Snapshot {
    previous
        .iter()
        .map(|(path, stamp)| (path.clone(), stamp.refresh(path)))
        .collect()
}

/// Expand the closure before trusting an analysis. A newly discovered path needs another
/// pass so its contents AND symlink target were observed before the loader used it.
fn after_analysis(sources: &SourceMap, before: &Snapshot, success: bool) -> (Snapshot, bool) {
    let source_paths = sources
        .files()
        .map(|file| file.path().to_owned())
        .collect::<std::collections::HashSet<_>>();
    let mut next = if success {
        Snapshot::new()
    } else {
        current(before)
    };
    for path in sources.dependencies() {
        let stamp = if source_paths.contains(path) {
            Stamp::contents(path)
        } else {
            // Resource dependencies can be tens of MiB. File identity, timestamps,
            // size, type, and permissions detect ordinary in-place and atomic saves
            // without copying every resource on every poll.
            Stamp::metadata(path)
        };
        next.insert(path.to_owned(), stamp);
    }
    let changed = next
        .iter()
        .any(|(path, stamp)| before.get(path) != Some(stamp))
        || sources.files().any(|file| {
            next.get(file.path())
                .and_then(Stamp::observed_contents)
                .is_none_or(|contents| contents.as_deref() != Ok(file.text().as_bytes()))
        });
    (next, changed)
}

pub fn run(
    path: &Path,
    environment: &toolchain::BuildEnvironment,
    options: &Options,
) -> Result<(), String> {
    let mut session = Session::start(options)?;
    let environment = environment.with_cancellation(session.cancelled.clone());
    let mut snapshot = Snapshot::from([(path.to_owned(), Stamp::contents(path))]);
    let mut rebuild = true;
    let mut changed_at = None;
    println!("Watching source dependencies; saves restart the game. Press Ctrl-C to stop.");
    while !session.is_cancelled() {
        session.check_server()?;
        if rebuild {
            stop_for_edit(&mut session);
            session.frames.set_state(ViewerState::Building);
            rebuild = false;
            changed_at = None;
            let before = current(&snapshot);
            match analyze_path(path) {
                Ok(program) => {
                    let (next, changed) = after_analysis(program.sources(), &before, true);
                    snapshot = next;
                    if changed {
                        rebuild = true;
                        thread::sleep(POLL);
                        continue;
                    }
                    let llvm = codegen::llvm::emit_for_development(
                        &program,
                        Some(environment.llvm_target_triple()),
                    );
                    let result = toolchain::build_for_development(
                        path,
                        &llvm,
                        &environment,
                        program.ast().resolution,
                    );
                    // An edit during analysis/native compilation must never be marked as compiled.
                    if current(&snapshot) != snapshot {
                        rebuild = true;
                        continue;
                    }
                    if session.is_cancelled() {
                        break;
                    }
                    match result {
                        Ok(artifacts) => session.launch(&artifacts.executable, options)?,
                        Err(error) => {
                            eprintln!("error: {error}");
                            session.frames.set_state(ViewerState::BuildFailed);
                        }
                    }
                }
                Err(error) => {
                    let (next, changed) = after_analysis(&error.sources, &before, false);
                    snapshot = next;
                    rebuild = changed;
                    eprintln!("{error}");
                    session.frames.set_state(ViewerState::BuildFailed);
                }
            }
        }
        if session.has_game() {
            match session.poll_game() {
                Ok(Some(status)) => {
                    stop_for_edit(&mut session);
                    if !status.success() {
                        eprintln!("development game exited with {status}");
                    }
                    session.frames.set_state(ViewerState::Watching);
                }
                Ok(None) => {}
                Err(error) => {
                    let _ = session.stop_game();
                    eprintln!("error: {error}");
                    session.frames.set_state(ViewerState::Watching);
                }
            }
        }
        let next = current(&snapshot);
        if next != snapshot {
            // Stop immediately, including the debounce interval and invalid intermediate saves.
            stop_for_edit(&mut session);
            session.frames.set_state(ViewerState::Building);
            snapshot = next;
            changed_at = Some(Instant::now());
        }
        if changed_at.is_some_and(|time| time.elapsed() >= QUIET) {
            rebuild = true;
        }
        thread::sleep(POLL);
    }
    session.check_server()?;
    session.stop_game()?;
    println!("Development watcher stopped cleanly after Ctrl-C.");
    Ok(())
}

fn stop_for_edit(session: &mut Session) {
    if let Err(error) = session.stop_game() {
        eprintln!("error: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[test]
    fn resource_stamps_do_not_retain_contents_and_detect_in_place_edits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.wav");
        let file = fs::File::create(&path).unwrap();
        file.set_len(16 * 1024 * 1024).unwrap();

        let first = Stamp::metadata(&path);
        assert!(matches!(
            first.observation,
            Observation::Metadata(Ok(MetadataStamp {
                length: 16_777_216,
                ..
            }))
        ));
        assert_eq!(first.refresh(&path), first);

        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[0])
            .unwrap();
        assert_ne!(first.refresh(&path), first);
    }

    #[cfg(unix)]
    #[test]
    fn resource_stamps_detect_symlink_retargets_with_equal_length_targets() {
        let directory = tempfile::tempdir().unwrap();
        let first_target = directory.path().join("first.wav");
        let second_target = directory.path().join("second.wav");
        let resource = directory.path().join("track.wav");
        fs::write(&first_target, b"one").unwrap();
        fs::write(&second_target, b"two").unwrap();
        symlink(&first_target, &resource).unwrap();
        let first = Stamp::metadata(&resource);

        fs::remove_file(&resource).unwrap();
        symlink(&second_target, &resource).unwrap();
        assert_ne!(first.refresh(&resource), first);
    }
}
