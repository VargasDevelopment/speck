//! The viewer outlives individual game processes; every owned thread/child has a drop path.
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{Options, server, viewer_url};

pub(super) struct Session {
    pub frames: server::FrameStore,
    pub cancelled: Arc<AtomicBool>,
    interrupted: Arc<AtomicBool>,
    controls: server::InputControl,
    threads: Vec<JoinHandle<()>>,
    fatal: mpsc::Receiver<String>,
    game: Option<GameRun>,
}

impl Session {
    pub fn start(options: &Options) -> Result<Self, String> {
        let http = server::bind_http(options.bind, options.port, !options.port_explicit)?;
        let frames = server::FrameStore::default();
        let controls = server::InputControl::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupt_flag = interrupted.clone();
        ctrlc::try_set_handler(move || {
            interrupt_flag.store(true, Ordering::Release);
            flag.store(true, Ordering::Release);
        })
        .map_err(|error| format!("could not install Ctrl-C handler: {error}"))?;
        let (fatal_tx, fatal) = mpsc::channel();
        let threads = vec![
            server::spawn_http_server(
                http.listener,
                frames.clone(),
                controls.clone(),
                cancelled.clone(),
                fatal_tx,
            ),
            server::spawn_input_watchdog(controls.clone(), cancelled.clone()),
        ];
        if http.used_fallback_port {
            println!(
                "Port {} was unavailable; selected safe fallback port {}.",
                options.port,
                http.address.port()
            );
        }
        if !options.bind.is_loopback() {
            println!(
                "Warning: development viewer explicitly bound to non-loopback address {}.",
                options.bind
            );
        }
        println!("Viewer URL: {}", viewer_url(http.address));
        if options.bind.is_loopback() {
            println!(
                "Remote access: ssh -L {0}:localhost:{0} <anfibio-host>",
                http.address.port()
            );
            println!("Then open: http://localhost:{}/", http.address.port());
        }
        match options.frame_limit {
            Some(limit) => println!("Frame limit: {limit}"),
            None => println!("Frame limit: unbounded (use Speck `quit()` or press Ctrl-C to stop)"),
        }
        Ok(Self {
            frames,
            controls,
            cancelled,
            interrupted,
            threads,
            fatal,
            game: None,
        })
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Acquire)
    }

    pub fn check_server(&self) -> Result<(), String> {
        match self.fatal.try_recv() {
            Ok(error) => Err(error),
            Err(_) => Ok(()),
        }
    }

    pub fn launch(&mut self, executable: &Path, options: &Options) -> Result<(), String> {
        debug_assert!(self.game.is_none());
        let listener = server::bind_frame_listener()?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("could not inspect frame receiver: {error}"))?
            .port();
        let mut command = Command::new(executable);
        command
            .env("SPECK_FRAME_STREAM_PORT", port.to_string())
            .env_remove("SPECK_FRAME_LIMIT")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if let Some(limit) = options.frame_limit {
            command.env("SPECK_FRAME_LIMIT", limit.to_string());
        }
        let child = command.spawn().map_err(|error| {
            format!(
                "could not launch development game `{}`: {error}",
                executable.display()
            )
        })?;
        let stop = Arc::new(AtomicBool::new(false));
        let (errors, fatal) = mpsc::channel();
        self.frames.begin_run();
        let receiver = server::spawn_frame_receiver(
            listener,
            self.frames.clone(),
            self.controls.clone(),
            stop.clone(),
            errors,
        );
        println!(
            "Development game: {} (pid {})",
            executable.display(),
            child.id()
        );
        self.game = Some(GameRun {
            child,
            stop,
            receiver: Some(receiver),
            fatal,
        });
        Ok(())
    }

    /// Poll without waiting; a receiver failure ends this run, not its viewer.
    pub fn poll_game(&mut self) -> Result<Option<ExitStatus>, String> {
        let Some(game) = &mut self.game else {
            return Ok(None);
        };
        if let Ok(error) = game.fatal.try_recv() {
            return Err(error);
        }
        game.child
            .try_wait()
            .map_err(|error| format!("could not inspect development game: {error}"))
    }

    pub fn has_game(&self) -> bool {
        self.game.is_some()
    }

    /// Stop before rebuilding or returning. Join the old receiver before a new generation begins.
    pub fn stop_game(&mut self) -> Result<(), String> {
        let Some(mut game) = self.game.take() else {
            return Ok(());
        };
        // The process is stopping: detach input without queuing a control record it may
        // never consume. No request can claim or write to this run after this boundary.
        self.controls.disconnect_game();
        if game
            .child
            .try_wait()
            .map_err(|error| format!("could not inspect development game: {error}"))?
            .is_none()
        {
            if let Err(error) = super::interrupt_child(&game.child)
                && game
                    .child
                    .try_wait()
                    .map_err(|failure| format!("could not inspect development game: {failure}"))?
                    .is_none()
            {
                return Err(error);
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while game
                .child
                .try_wait()
                .map_err(|error| format!("could not inspect development game: {error}"))?
                .is_none()
            {
                if Instant::now() >= deadline {
                    return Err("development game did not stop within two seconds of Ctrl-C".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        game.stop.store(true, Ordering::Release);
        game.receiver
            .take()
            .unwrap()
            .join()
            .map_err(|_| "frame receiver thread panicked")?;
        match game.fatal.try_recv() {
            Ok(error) => Err(error),
            Err(_) => Ok(()),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Also protects early errors before the ordinary graceful stop path.
        drop(self.game.take());
        self.cancelled.store(true, Ordering::Release);
        self.frames.stop();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

struct GameRun {
    child: Child,
    stop: Arc<AtomicBool>,
    receiver: Option<JoinHandle<()>>,
    fatal: mpsc::Receiver<String>,
}

impl Drop for GameRun {
    fn drop(&mut self) {
        super::terminate(&mut self.child);
        self.stop.store(true, Ordering::Release);
        if let Some(receiver) = self.receiver.take() {
            let _ = receiver.join();
        }
    }
}
