use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tracing::{error, warn};
use uc_core::settings::model::QuickPanelDoubleTapModifier;

use crate::modifier_double_tap::ModifierDoubleTapDetector;

// Poll only while the trigger is enabled. A 20 ms interval keeps recognition
// latency near one display frame while bounding the active cost at 50 keyboard
// snapshots per second; disable and shutdown release the worker immediately.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const WORKER_START_TIMEOUT: Duration = Duration::from_secs(2);

/// Supplies one platform keyboard snapshot to the framework-neutral monitor.
pub trait ModifierKeyState: Send {
    fn snapshot(&mut self, modifier: QuickPanelDoubleTapModifier) -> (bool, bool);
}

pub type ModifierKeyStateFactory =
    Arc<dyn Fn() -> Result<Box<dyn ModifierKeyState>, String> + Send + Sync>;

enum WorkerCommand {
    SetModifier(QuickPanelDoubleTapModifier),
    Shutdown,
}

struct Worker {
    sender: mpsc::Sender<WorkerCommand>,
    join_handle: Option<JoinHandle<()>>,
}

struct MonitorState {
    current: QuickPanelDoubleTapModifier,
    worker: Option<Worker>,
}

/// Owns the process-lifetime keyboard snapshot worker for modifier-only taps.
///
/// The worker exists only while a modifier trigger is active. Disabling the
/// trigger or shutting down the desktop host releases the keyboard backend and
/// joins the worker thread synchronously.
#[derive(Clone)]
pub struct ModifierDoubleTapMonitor {
    inner: Arc<MonitorInner>,
}

struct MonitorInner {
    state: Mutex<MonitorState>,
    key_state_factory: ModifierKeyStateFactory,
    on_trigger: Arc<dyn Fn() + Send + Sync>,
}

impl ModifierDoubleTapMonitor {
    pub fn new(
        key_state_factory: ModifierKeyStateFactory,
        on_trigger: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(MonitorInner {
                state: Mutex::new(MonitorState {
                    current: QuickPanelDoubleTapModifier::Disabled,
                    worker: None,
                }),
                key_state_factory,
                on_trigger: Arc::new(on_trigger),
            }),
        }
    }

    pub fn current(&self) -> QuickPanelDoubleTapModifier {
        self.lock_state().current
    }

    pub fn set_modifier(&self, modifier: QuickPanelDoubleTapModifier) -> Result<(), String> {
        if modifier == QuickPanelDoubleTapModifier::Disabled {
            self.shutdown();
            return Ok(());
        }

        let mut state = self.lock_state();
        if state.worker.is_none() {
            state.worker = Some(start_worker(
                Arc::clone(&self.inner.key_state_factory),
                Arc::clone(&self.inner.on_trigger),
            )?);
        }

        let send_result = state
            .worker
            .as_ref()
            .ok_or_else(|| "modifier double-tap worker was not created".to_string())?
            .sender
            .send(WorkerCommand::SetModifier(modifier));

        match send_result {
            Ok(()) => {
                state.current = modifier;
                Ok(())
            }
            Err(error) => {
                let worker = state.worker.take();
                state.current = QuickPanelDoubleTapModifier::Disabled;
                drop(state);
                if let Some(worker) = worker {
                    stop_worker(worker);
                }
                Err(format!("modifier double-tap worker stopped: {error}"))
            }
        }
    }

    /// Stop the worker and release the platform keyboard backend.
    ///
    /// Idempotent so explicit host shutdown and `Drop` can share the same path.
    pub fn shutdown(&self) {
        let worker = {
            let mut state = self.lock_state();
            state.current = QuickPanelDoubleTapModifier::Disabled;
            state.worker.take()
        };
        if let Some(worker) = worker {
            stop_worker(worker);
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, MonitorState> {
        match self.inner.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                error!("Modifier double-tap monitor lock poisoned; recovering");
                poisoned.into_inner()
            }
        }
    }
}

impl Drop for MonitorInner {
    fn drop(&mut self) {
        let worker = match self.state.get_mut() {
            Ok(state) => {
                state.current = QuickPanelDoubleTapModifier::Disabled;
                state.worker.take()
            }
            Err(poisoned) => poisoned.into_inner().worker.take(),
        };
        if let Some(worker) = worker {
            stop_worker(worker);
        }
    }
}

fn stop_worker(mut worker: Worker) {
    let _ = worker.sender.send(WorkerCommand::Shutdown);
    if let Some(join_handle) = worker.join_handle.take() {
        if join_handle.join().is_err() {
            warn!("Modifier double-tap worker panicked during shutdown");
        }
    }
}

fn start_worker(
    key_state_factory: ModifierKeyStateFactory,
    on_trigger: Arc<dyn Fn() + Send + Sync>,
) -> Result<Worker, String> {
    let (command_tx, command_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let join_handle = thread::Builder::new()
        .name("quick-panel-modifier-tap".to_string())
        .spawn(move || {
            let mut key_state = match key_state_factory() {
                Ok(key_state) => key_state,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            run_worker(key_state.as_mut(), command_rx, on_trigger);
        })
        .map_err(|error| format!("failed to spawn modifier double-tap worker: {error}"))?;

    match ready_rx.recv_timeout(WORKER_START_TIMEOUT) {
        Ok(Ok(())) => Ok(Worker {
            sender: command_tx,
            join_handle: Some(join_handle),
        }),
        Ok(Err(error)) => {
            let _ = join_handle.join();
            Err(error)
        }
        Err(error) => {
            let _ = command_tx.send(WorkerCommand::Shutdown);
            // The platform constructor may still be blocked. Dropping the
            // handle detaches the thread; once construction returns it observes
            // Shutdown before sampling, so the caller remains time-bounded.
            drop(join_handle);
            Err(format!(
                "modifier double-tap worker did not initialize in time: {error}"
            ))
        }
    }
}

fn run_worker(
    key_state: &mut dyn ModifierKeyState,
    command_rx: mpsc::Receiver<WorkerCommand>,
    on_trigger: Arc<dyn Fn() + Send + Sync>,
) {
    let mut selected_modifier = QuickPanelDoubleTapModifier::Disabled;
    let mut detector = ModifierDoubleTapDetector::default();

    loop {
        match command_rx.recv_timeout(POLL_INTERVAL) {
            Ok(WorkerCommand::SetModifier(modifier)) => {
                selected_modifier = modifier;
                detector = ModifierDoubleTapDetector::default();
            }
            Ok(WorkerCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                observe_keyboard(key_state, selected_modifier, &mut detector, &on_trigger);
            }
        }
    }
}

fn observe_keyboard(
    key_state: &mut dyn ModifierKeyState,
    modifier: QuickPanelDoubleTapModifier,
    detector: &mut ModifierDoubleTapDetector,
    on_trigger: &Arc<dyn Fn() + Send + Sync>,
) {
    let (selected_down, other_down) = key_state.snapshot(modifier);
    if detector.observe(selected_down, other_down, Instant::now()) {
        on_trigger();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyKeyState;

    impl ModifierKeyState for EmptyKeyState {
        fn snapshot(&mut self, _modifier: QuickPanelDoubleTapModifier) -> (bool, bool) {
            (false, false)
        }
    }

    #[test]
    fn shutdown_without_worker_is_idempotent() {
        let factory: ModifierKeyStateFactory = Arc::new(|| Ok(Box::new(EmptyKeyState)));
        let monitor = ModifierDoubleTapMonitor::new(factory, || {});
        monitor.shutdown();
        monitor.shutdown();
        assert_eq!(monitor.current(), QuickPanelDoubleTapModifier::Disabled);
    }
}
