//! Supervision of the native quick panel helper process.
//!
//! The GUI starts the helper when the quick panel is enabled and stops it when the panel is
//! disabled or the GUI exits. The helper is a separate process because a GPUI application and the
//! GUI's own windowing runtime each need to own the main thread's event loop.
//!
//! This module is independent of any GUI framework. [`HelperSupervisor`] is the state machine
//! (what should be running, and when to retry a helper that keeps failing); [`ProcessLauncher`] is
//! the real process implementation.

use std::io;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// Command line flag telling the helper to exit once its standard input is closed.
///
/// The supervisor keeps the write end open while it runs, so an unclean GUI exit (crash, kill)
/// still ends the helper: the operating system closes the pipe.
pub const EXIT_WHEN_STDIN_CLOSES: &str = "--exit-when-stdin-closes";

/// File stem of the helper executable that ships next to the GUI.
pub const HELPER_EXE_STEM: &str = "uniclip-quick-panel";

/// Base of the exponential delay before restarting a helper that exited unexpectedly.
const RESTART_BASE_DELAY: Duration = Duration::from_secs(1);
const RESTART_MAX_DELAY: Duration = Duration::from_secs(30);
/// A helper that stayed up this long counts as healthy, so earlier failures are forgotten.
const STABLE_AFTER: Duration = Duration::from_secs(60);
/// Consecutive short-lived runs after which the supervisor stops retrying.
const MAX_CONSECUTIVE_FAILURES: u32 = 5;
/// How long a helper gets to exit on its own after its input is closed before it is killed.
const GRACEFUL_EXIT: Duration = Duration::from_secs(1);
/// How often the driver thread checks on the helper.
const TICK_INTERVAL: Duration = Duration::from_millis(500);

/// What the helper asks the GUI to do. The helper has no main window of its own, so anything that
/// belongs to the GUI (unlocking, settings, the history page) goes through these requests.
///
/// On the wire each request is one line of JSON on the helper's standard output, for example
/// `{"request":"open_settings"}`. Lines that are not requests (log noise, a newer helper's
/// requests) are ignored, so the two sides can be updated one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperRequest {
    /// Bring the main window to the front. The lock screen there is how the content is unlocked.
    ShowMainWindow,
    /// Bring the main window to the front on its settings page.
    OpenSettings,
}

impl HelperRequest {
    const fn wire_name(self) -> &'static str {
        match self {
            Self::ShowMainWindow => "show_main_window",
            Self::OpenSettings => "open_settings",
        }
    }

    /// The line the helper writes for this request, without the newline.
    pub fn to_line(self) -> String {
        format!(r#"{{"request":"{}"}}"#, self.wire_name())
    }

    /// Reads one line of the helper's output; `None` for anything that is not a known request.
    pub fn from_line(line: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        match value.get("request")?.as_str()? {
            "show_main_window" => Some(Self::ShowMainWindow),
            "open_settings" => Some(Self::OpenSettings),
            _ => None,
        }
    }
}

/// Receives the requests of a helper. It runs on a reader thread, so it must not block for long.
pub type RequestHandler = Arc<dyn Fn(HelperRequest) + Send + Sync>;

/// A running helper.
pub trait HelperChild: Send {
    /// Returns `true` once the helper has exited.
    fn has_exited(&mut self) -> bool;
    /// Stops the helper and waits for it, force-killing it if it does not leave promptly.
    fn terminate(&mut self);
}

/// Starts helper processes.
pub trait HelperLauncher: Send {
    fn launch(&mut self) -> io::Result<Box<dyn HelperChild>>;
}

struct Running {
    child: Box<dyn HelperChild>,
    started_at: Instant,
}

/// Decides whether a helper should be running and restarts it after unexpected exits.
///
/// All time is passed in, so the retry policy is testable without waiting.
pub struct HelperSupervisor<L: HelperLauncher> {
    launcher: L,
    enabled: bool,
    running: Option<Running>,
    failures: u32,
    retry_at: Option<Instant>,
    gave_up: bool,
}

impl<L: HelperLauncher> HelperSupervisor<L> {
    pub fn new(launcher: L) -> Self {
        Self {
            launcher,
            enabled: false,
            running: None,
            failures: 0,
            retry_at: None,
            gave_up: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// True after repeated short-lived runs made the supervisor stop retrying.
    pub fn has_given_up(&self) -> bool {
        self.gave_up
    }

    /// Sets whether the helper should run. Enabling starts it right away; disabling stops it.
    pub fn set_enabled(&mut self, enabled: bool, now: Instant) {
        self.enabled = enabled;
        if enabled {
            self.reset_failures();
            self.start(now);
        } else {
            self.stop();
        }
    }

    /// Stops the helper and, if enabled, starts a fresh one. Used when a setting the helper reads
    /// only at startup changes.
    pub fn restart(&mut self, now: Instant) {
        self.stop();
        if self.enabled {
            self.reset_failures();
            self.start(now);
        }
    }

    /// Stops the helper for good, for example when the GUI exits.
    pub fn shutdown(&mut self) {
        self.enabled = false;
        self.stop();
    }

    /// Notices an exited helper and starts a replacement once its retry delay has passed.
    pub fn tick(&mut self, now: Instant) {
        if let Some(running) = self.running.as_mut() {
            if !running.child.has_exited() {
                return;
            }
            let ran_for = now.saturating_duration_since(running.started_at);
            self.running = None;
            if self.enabled {
                self.record_failure(ran_for, now);
            }
        }
        if self.enabled && !self.gave_up && self.running.is_none() {
            if let Some(retry_at) = self.retry_at {
                if now >= retry_at {
                    self.start(now);
                }
            }
        }
    }

    fn start(&mut self, now: Instant) {
        if self.running.is_some() {
            return;
        }
        self.retry_at = None;
        match self.launcher.launch() {
            Ok(child) => {
                info!("Quick panel helper started");
                self.running = Some(Running {
                    child,
                    started_at: now,
                });
            }
            Err(error) => {
                warn!(error = %error, "Failed to start the quick panel helper");
                self.record_failure(Duration::ZERO, now);
            }
        }
    }

    fn stop(&mut self) {
        self.retry_at = None;
        if let Some(mut running) = self.running.take() {
            running.child.terminate();
            info!("Quick panel helper stopped");
        }
    }

    fn reset_failures(&mut self) {
        self.failures = 0;
        self.gave_up = false;
        self.retry_at = None;
    }

    fn record_failure(&mut self, ran_for: Duration, now: Instant) {
        if ran_for >= STABLE_AFTER {
            self.failures = 0;
        }
        self.failures += 1;
        if self.failures > MAX_CONSECUTIVE_FAILURES {
            self.gave_up = true;
            self.retry_at = None;
            warn!("Quick panel helper keeps failing; not restarting it again");
            return;
        }
        let exponent = (self.failures - 1).min(16);
        let delay = RESTART_BASE_DELAY
            .saturating_mul(1 << exponent)
            .min(RESTART_MAX_DELAY);
        self.retry_at = Some(now + delay);
        warn!(
            delay_ms = delay.as_millis() as u64,
            "Quick panel helper exited; restarting it"
        );
    }
}

/// Shared handle that keeps a supervisor ticking on a background thread.
pub struct SupervisedHelper<L: HelperLauncher + 'static> {
    supervisor: Arc<Mutex<HelperSupervisor<L>>>,
    stop: Arc<Mutex<bool>>,
    driver: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<L: HelperLauncher + 'static> SupervisedHelper<L> {
    pub fn new(launcher: L) -> Self {
        let supervisor = Arc::new(Mutex::new(HelperSupervisor::new(launcher)));
        let stop = Arc::new(Mutex::new(false));
        let driver = {
            let supervisor = supervisor.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("quick-panel-helper".into())
                .spawn(move || loop {
                    std::thread::sleep(TICK_INTERVAL);
                    if *lock(&stop) {
                        return;
                    }
                    lock(&supervisor).tick(Instant::now());
                })
                .map_err(|error| warn!(error = %error, "Could not start the helper supervisor"))
                .ok()
        };
        Self {
            supervisor,
            stop,
            driver: Mutex::new(driver),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        lock(&self.supervisor).set_enabled(enabled, Instant::now());
    }

    pub fn restart(&self) {
        lock(&self.supervisor).restart(Instant::now());
    }

    pub fn is_running(&self) -> bool {
        lock(&self.supervisor).is_running()
    }

    /// Stops the helper and the driver thread. Safe to call more than once.
    pub fn shutdown(&self) {
        *lock(&self.stop) = true;
        lock(&self.supervisor).shutdown();
        if let Some(driver) = lock(&self.driver).take() {
            let _ = driver.join();
        }
    }
}

/// Resolves the helper executable next to the current executable (and the daemon).
pub fn resolve_helper_exe_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{HELPER_EXE_STEM}.exe")
    } else {
        HELPER_EXE_STEM.to_string()
    };
    let candidate = std::env::current_exe().ok()?.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}

/// Starts real helper processes with a piped standard input.
pub struct ProcessLauncher {
    executable: PathBuf,
    arguments: Vec<String>,
    on_request: Option<RequestHandler>,
}

impl ProcessLauncher {
    pub fn new(executable: PathBuf, arguments: Vec<String>) -> Self {
        Self {
            executable,
            arguments,
            on_request: None,
        }
    }

    /// Delivers the helper's requests to `handler`.
    pub fn with_request_handler(mut self, handler: RequestHandler) -> Self {
        self.on_request = Some(handler);
        self
    }

    /// The helper as shipped: it exits when the GUI goes away.
    pub fn for_helper(executable: PathBuf) -> Self {
        Self::new(executable, vec![EXIT_WHEN_STDIN_CLOSES.to_string()])
    }
}

impl HelperLauncher for ProcessLauncher {
    fn launch(&mut self) -> io::Result<Box<dyn HelperChild>> {
        let mut command = Command::new(&self.executable);
        command
            .args(&self.arguments)
            .stdin(Stdio::piped())
            .stdout(if self.on_request.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        if let (Some(handler), Some(output)) = (self.on_request.clone(), child.stdout.take()) {
            // Ends by itself when the helper exits and closes its output.
            let reader = std::thread::Builder::new()
                .name("quick-panel-requests".into())
                .spawn(move || {
                    for line in std::io::BufRead::lines(std::io::BufReader::new(output)) {
                        let Ok(line) = line else { break };
                        match HelperRequest::from_line(&line) {
                            Some(request) => handler(request),
                            None => tracing::debug!("Ignored a line of helper output"),
                        }
                    }
                });
            if let Err(error) = reader {
                warn!(error = %error, "Could not read the helper's requests");
            }
        }
        Ok(Box::new(ProcessChild { child, input }))
    }
}

struct ProcessChild {
    child: Child,
    /// Held open for the helper's lifetime; dropping it tells the helper to exit.
    input: Option<ChildStdin>,
}

impl HelperChild for ProcessChild {
    fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    fn terminate(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + GRACEFUL_EXIT;
        while Instant::now() < deadline {
            if self.has_exited() {
                let _ = self.child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn a_request_survives_the_round_trip_through_a_line() {
        for request in [HelperRequest::ShowMainWindow, HelperRequest::OpenSettings] {
            assert_eq!(HelperRequest::from_line(&request.to_line()), Some(request));
        }
    }

    #[test]
    fn lines_that_are_not_known_requests_are_ignored() {
        for line in [
            "",
            "plain log text",
            r#"{"request":"reboot_the_world"}"#,
            r#"{"other":"open_settings"}"#,
            r#"{"request":7}"#,
            "[1,2]",
        ] {
            assert_eq!(HelperRequest::from_line(line), None, "{line:?}");
        }
        // Surrounding whitespace, as in a CRLF line ending, is fine.
        assert_eq!(
            HelperRequest::from_line("  {\"request\":\"open_settings\"}\r\n"),
            Some(HelperRequest::OpenSettings)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_process_delivers_its_requests_and_noise_is_skipped() {
        let (send, receive) = std::sync::mpsc::channel();
        let send = Mutex::new(send);
        let mut launcher = ProcessLauncher::new(
            PathBuf::from("/bin/sh"),
            vec![
                "-c".into(),
                r#"echo noise; echo '{"request":"open_settings"}'; echo '{"request":"show_main_window"}'"#
                    .into(),
            ],
        )
        .with_request_handler(Arc::new(move |request| {
            let _ = lock(&send).send(request);
        }));
        let mut child = launcher.launch().unwrap();
        let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        let second = receive.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(first, HelperRequest::OpenSettings);
        assert_eq!(second, HelperRequest::ShowMainWindow);
        child.terminate();
    }

    #[derive(Default)]
    struct Shared {
        launches: AtomicU32,
        terminations: AtomicU32,
        fail_launch: Mutex<bool>,
        alive: Mutex<Vec<Arc<Mutex<bool>>>>,
    }

    struct FakeChild {
        alive: Arc<Mutex<bool>>,
        shared: Arc<Shared>,
    }

    impl HelperChild for FakeChild {
        fn has_exited(&mut self) -> bool {
            !*lock(&self.alive)
        }
        fn terminate(&mut self) {
            *lock(&self.alive) = false;
            self.shared.terminations.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct FakeLauncher(Arc<Shared>);

    impl HelperLauncher for FakeLauncher {
        fn launch(&mut self) -> io::Result<Box<dyn HelperChild>> {
            self.0.launches.fetch_add(1, Ordering::SeqCst);
            if *lock(&self.0.fail_launch) {
                return Err(io::Error::other("no such file"));
            }
            let alive = Arc::new(Mutex::new(true));
            lock(&self.0.alive).push(alive.clone());
            Ok(Box::new(FakeChild {
                alive,
                shared: self.0.clone(),
            }))
        }
    }

    fn setup() -> (Arc<Shared>, HelperSupervisor<FakeLauncher>, Instant) {
        let shared = Arc::new(Shared::default());
        let supervisor = HelperSupervisor::new(FakeLauncher(shared.clone()));
        (shared, supervisor, Instant::now())
    }

    fn crash_latest(shared: &Shared) {
        *lock(lock(&shared.alive).last().unwrap()) = false;
    }

    #[test]
    fn nothing_runs_until_the_panel_is_enabled() {
        let (shared, mut supervisor, now) = setup();
        supervisor.tick(now);
        assert!(!supervisor.is_running());
        assert_eq!(shared.launches.load(Ordering::SeqCst), 0);
        supervisor.set_enabled(true, now);
        assert!(supervisor.is_running());
        assert_eq!(shared.launches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn disabling_stops_the_helper_and_enabling_again_starts_a_new_one() {
        let (shared, mut supervisor, now) = setup();
        supervisor.set_enabled(true, now);
        supervisor.set_enabled(false, now);
        assert!(!supervisor.is_running());
        assert_eq!(shared.terminations.load(Ordering::SeqCst), 1);
        supervisor.tick(now + Duration::from_secs(60));
        assert!(
            !supervisor.is_running(),
            "a disabled panel is not restarted"
        );
        supervisor.set_enabled(true, now);
        assert!(supervisor.is_running());
        assert_eq!(shared.launches.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_crashed_helper_is_restarted_after_a_delay() {
        let (shared, mut supervisor, now) = setup();
        supervisor.set_enabled(true, now);
        crash_latest(&shared);
        supervisor.tick(now);
        assert!(!supervisor.is_running());
        supervisor.tick(now + Duration::from_millis(500));
        assert!(!supervisor.is_running(), "still inside the retry delay");
        supervisor.tick(now + Duration::from_secs(1));
        assert!(supervisor.is_running());
        assert_eq!(shared.launches.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn repeated_short_runs_back_off_and_then_stop() {
        let (shared, mut supervisor, mut now) = setup();
        supervisor.set_enabled(true, now);
        let mut delays = vec![];
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            crash_latest(&shared);
            supervisor.tick(now);
            let mut waited = Duration::ZERO;
            while !supervisor.is_running() && !supervisor.has_given_up() {
                waited += Duration::from_millis(250);
                supervisor.tick(now + waited);
                assert!(waited <= RESTART_MAX_DELAY);
            }
            delays.push(waited);
            now += waited;
        }
        assert_eq!(delays[0], Duration::from_secs(1));
        assert_eq!(delays[1], Duration::from_secs(2));
        assert_eq!(delays[2], Duration::from_secs(4));
        assert_eq!(delays[3], Duration::from_secs(8));
        crash_latest(&shared);
        supervisor.tick(now);
        assert!(supervisor.has_given_up());
        let launches = shared.launches.load(Ordering::SeqCst);
        supervisor.tick(now + Duration::from_secs(3600));
        assert_eq!(shared.launches.load(Ordering::SeqCst), launches);
        // Enabling again is an explicit request and clears the give-up state.
        supervisor.set_enabled(true, now);
        assert!(supervisor.is_running());
        assert!(!supervisor.has_given_up());
    }

    #[test]
    fn a_long_lived_helper_does_not_count_toward_giving_up() {
        let (shared, mut supervisor, mut now) = setup();
        supervisor.set_enabled(true, now);
        for _ in 0..(MAX_CONSECUTIVE_FAILURES * 2) {
            now += STABLE_AFTER;
            crash_latest(&shared);
            supervisor.tick(now);
            now += RESTART_BASE_DELAY;
            supervisor.tick(now);
            assert!(supervisor.is_running());
            assert!(!supervisor.has_given_up());
        }
    }

    #[test]
    fn a_missing_executable_backs_off_instead_of_spinning() {
        let (shared, mut supervisor, mut now) = setup();
        *lock(&shared.fail_launch) = true;
        supervisor.set_enabled(true, now);
        for _ in 0..100 {
            now += Duration::from_secs(60);
            supervisor.tick(now);
        }
        assert!(supervisor.has_given_up());
        assert_eq!(
            shared.launches.load(Ordering::SeqCst),
            MAX_CONSECUTIVE_FAILURES + 1
        );
    }

    #[test]
    fn restart_replaces_a_running_helper_only_while_enabled() {
        let (shared, mut supervisor, now) = setup();
        supervisor.restart(now);
        assert_eq!(shared.launches.load(Ordering::SeqCst), 0);
        supervisor.set_enabled(true, now);
        supervisor.restart(now);
        assert_eq!(shared.launches.load(Ordering::SeqCst), 2);
        assert_eq!(shared.terminations.load(Ordering::SeqCst), 1);
        assert!(supervisor.is_running());
    }

    #[test]
    fn shutdown_stops_the_helper_and_it_stays_stopped() {
        let (shared, mut supervisor, now) = setup();
        supervisor.set_enabled(true, now);
        supervisor.shutdown();
        assert!(!supervisor.is_running());
        supervisor.tick(now + Duration::from_secs(3600));
        assert!(!supervisor.is_running());
        assert_eq!(shared.launches.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_real_process_exits_when_its_input_is_closed() {
        // `cat` copies its input until it is closed, like the helper waiting on its parent.
        let mut launcher = ProcessLauncher::new("/bin/cat".into(), vec![]);
        let mut child = launcher.launch().unwrap();
        assert!(!child.has_exited());
        let started = Instant::now();
        child.terminate();
        assert!(child.has_exited());
        assert!(
            started.elapsed() < GRACEFUL_EXIT,
            "closing the input should end it without a forced kill"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_process_that_ignores_its_input_is_killed() {
        let mut launcher = ProcessLauncher::new("/bin/sleep".into(), vec!["60".into()]);
        let mut child = launcher.launch().unwrap();
        child.terminate();
        assert!(child.has_exited());
    }

    #[test]
    fn a_missing_helper_executable_is_a_launch_error() {
        let mut launcher = ProcessLauncher::for_helper("/nonexistent/uniclip-quick-panel".into());
        assert!(launcher.launch().is_err());
    }
}
