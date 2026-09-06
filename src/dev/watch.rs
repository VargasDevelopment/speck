//! Poll the analyzed dependency closure and restart whole games on stable edits.
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::{Options, server::ViewerState, session::Session};
use crate::source::SourceMap;
use crate::{analyze_path, codegen, toolchain};

const POLL: Duration = Duration::from_millis(100);
const QUIET: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    target: Option<PathBuf>,
    bytes: Result<Vec<u8>, io::ErrorKind>,
}

impl Stamp {
    fn read(path: &Path) -> Self {
        Self {
            target: path.canonicalize().ok(),
            bytes: fs::read(path).map_err(|error| error.kind()),
        }
    }
}

type Snapshot = BTreeMap<PathBuf, Stamp>;

fn current(previous: &Snapshot) -> Snapshot {
    previous
        .keys()
        .map(|path| (path.clone(), Stamp::read(path)))
        .collect()
}

/// Expand the closure before trusting an analysis. A newly discovered path needs another
/// pass so its contents AND symlink target were observed before the loader used it.
fn after_analysis(sources: &SourceMap, before: &Snapshot, success: bool) -> (Snapshot, bool) {
    let mut next = if success {
        Snapshot::new()
    } else {
        current(before)
    };
    for path in sources.dependencies() {
        next.insert(path.to_owned(), Stamp::read(path));
    }
    let changed = next
        .iter()
        .any(|(path, stamp)| before.get(path) != Some(stamp))
        || sources.files().any(|file| {
            next.get(file.path())
                .is_none_or(|stamp| stamp.bytes.as_deref() != Ok(file.text().as_bytes()))
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
    let mut snapshot = Snapshot::from([(path.to_owned(), Stamp::read(path))]);
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
                    let result = toolchain::build_for_development(path, &llvm, &environment);
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
