use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub(crate) mod event_stream;
mod in_flight;

use crate::runtime::ProductionRuntime;
#[cfg(feature = "dev-tools")]
use crate::{DevOperation, DevOperationResult, DevPairingOutcomeStream};
use crate::{
    EngineConfig, EngineError, EngineErrorCategory, EngineEvent, EngineState, HostCapabilities,
    LifecycleAction, Operation, OperationResult, OperationTerminal,
};
pub use event_stream::EventStream;
use event_stream::{event_channel, EventSender};
use in_flight::InFlightOperations;

const INVALID_STATE_CODE: u32 = 1001;
const OPERATION_CANCELLED_CODE: u32 = 1002;

#[async_trait]
pub(crate) trait EngineRuntime: Send + Sync {
    async fn execute(
        &self,
        operation: Operation,
        cancellation: CancellationToken,
    ) -> Result<OperationResult, EngineError>;

    #[cfg(feature = "dev-tools")]
    async fn execute_dev(
        &self,
        _operation: DevOperation,
        _cancellation: CancellationToken,
    ) -> Result<DevOperationResult, EngineError> {
        Err(EngineError::new(
            1901,
            EngineErrorCategory::Unavailable,
            false,
        ))
    }

    #[cfg(feature = "dev-tools")]
    async fn dev_pairing_outcomes(&self) -> Result<DevPairingOutcomeStream, EngineError> {
        Err(EngineError::new(
            1902,
            EngineErrorCategory::Unavailable,
            false,
        ))
    }

    async fn suspend(&self) -> Result<(), EngineError>;
    async fn resume(&self) -> Result<(), EngineError>;
    async fn shutdown(&self, deadline: Duration) -> Result<(), EngineError>;
}

pub struct Engine {
    state: Mutex<EngineState>,
    lifecycle_gate: Mutex<()>,
    runtime: Arc<dyn EngineRuntime>,
    events: EventSender,
    operations: InFlightOperations,
}

impl Engine {
    pub async fn start(
        config: EngineConfig,
        host: HostCapabilities,
    ) -> Result<(Self, EventStream), EngineError> {
        const EVENT_CAPACITY: usize = 256;

        let (events, stream) = event_channel(EVENT_CAPACITY);
        let runtime = Arc::new(ProductionRuntime::start(config, host, events.clone()).await?);
        let engine = Self {
            state: Mutex::new(EngineState::Running),
            lifecycle_gate: Mutex::new(()),
            runtime,
            events,
            operations: InFlightOperations::new(),
        };
        engine.events.send(EngineEvent::StateChanged {
            state: EngineState::Running,
        });
        Ok((engine, stream))
    }

    #[cfg(test)]
    fn from_runtime<R>(runtime: Arc<R>, event_capacity: usize) -> (Self, EventStream)
    where
        R: EngineRuntime + 'static,
    {
        let (events, stream) = event_channel(event_capacity);
        (
            Self {
                state: Mutex::new(EngineState::Running),
                lifecycle_gate: Mutex::new(()),
                runtime,
                events,
                operations: InFlightOperations::new(),
            },
            stream,
        )
    }

    pub async fn lifecycle_state(&self) -> EngineState {
        *self.state.lock().await
    }

    pub async fn execute(&self, operation: Operation) -> Result<OperationResult, EngineError> {
        let registered = {
            let _lifecycle = self.lifecycle_gate.lock().await;
            if !self.state.lock().await.accepts_operations() {
                return Err(invalid_state_error());
            }
            self.operations.register("operation").await
        };

        let result = tokio::select! {
            _ = registered.cancellation.cancelled() => Err(operation_cancelled_error()),
            result = self.runtime.execute(operation, registered.cancellation.clone()) => result,
        };

        let should_emit_terminal = self.operations.finish(&registered.id).await;
        if should_emit_terminal {
            self.events.send(EngineEvent::OperationFinished {
                operation_id: registered.id,
                terminal: terminal_for_result(&result),
            });
        }

        result
    }

    #[cfg(feature = "dev-tools")]
    pub async fn execute_dev(
        &self,
        operation: DevOperation,
    ) -> Result<DevOperationResult, EngineError> {
        let registered = {
            let _lifecycle = self.lifecycle_gate.lock().await;
            if !self.state.lock().await.accepts_operations() {
                return Err(invalid_state_error());
            }
            self.operations.register("dev-operation").await
        };

        let result = tokio::select! {
            _ = registered.cancellation.cancelled() => Err(operation_cancelled_error()),
            result = self.runtime.execute_dev(operation, registered.cancellation.clone()) => result,
        };

        let should_emit_terminal = self.operations.finish(&registered.id).await;
        if should_emit_terminal {
            self.events.send(EngineEvent::OperationFinished {
                operation_id: registered.id,
                terminal: terminal_for_result(&result),
            });
        }
        result
    }

    #[cfg(feature = "dev-tools")]
    pub async fn dev_pairing_outcomes(&self) -> Result<DevPairingOutcomeStream, EngineError> {
        if !self.state.lock().await.accepts_operations() {
            return Err(invalid_state_error());
        }
        self.runtime.dev_pairing_outcomes().await
    }

    pub async fn quiesce(&self, deadline: Duration) -> Result<(), EngineError> {
        let _lifecycle = self.lifecycle_gate.lock().await;
        self.quiesce_locked(deadline).await
    }

    pub async fn suspend(&self) -> Result<(), EngineError> {
        let _lifecycle = self.lifecycle_gate.lock().await;
        let lifecycle = *self.state.lock().await;
        match lifecycle {
            EngineState::Running => self.quiesce_locked(Duration::ZERO).await?,
            EngineState::Quiesced => {}
            _ => return Err(invalid_state_error()),
        }

        self.report_lifecycle_result(LifecycleAction::Suspend, self.runtime.suspend().await)?;
        *self.state.lock().await = EngineState::Suspended;
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::Suspended,
        });
        Ok(())
    }

    pub async fn resume(&self) -> Result<(), EngineError> {
        let _lifecycle = self.lifecycle_gate.lock().await;
        if *self.state.lock().await != EngineState::Suspended {
            return Err(invalid_state_error());
        }

        self.report_lifecycle_result(LifecycleAction::Resume, self.runtime.resume().await)?;
        *self.state.lock().await = EngineState::Running;
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::Running,
        });
        Ok(())
    }

    pub async fn shutdown(&self, deadline: Duration) -> Result<(), EngineError> {
        let _lifecycle = self.lifecycle_gate.lock().await;
        let deadline_at = tokio::time::Instant::now() + deadline;
        let lifecycle = *self.state.lock().await;
        match lifecycle {
            EngineState::Running => {
                self.quiesce_locked(remaining_until(deadline_at)).await?;
            }
            EngineState::Quiesced | EngineState::Suspended => {}
            _ => return Err(invalid_state_error()),
        }

        *self.state.lock().await = EngineState::ShuttingDown;
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::ShuttingDown,
        });

        let shutdown_result = match tokio::time::timeout(
            remaining_until(deadline_at),
            self.runtime.shutdown(remaining_until(deadline_at)),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(operation_cancelled_error()),
        };

        if let Err(error) = &shutdown_result {
            if !error.is_retryable() {
                self.events.send(EngineEvent::Fatal {
                    error: error.clone(),
                });
            }
        }
        *self.state.lock().await = EngineState::Stopped;
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::Stopped,
        });
        self.events.close();
        shutdown_result
    }

    async fn quiesce_locked(&self, deadline: Duration) -> Result<(), EngineError> {
        {
            let mut state = self.state.lock().await;
            if *state != EngineState::Running {
                return Err(invalid_state_error());
            }
            *state = EngineState::Quiescing;
        }
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::Quiescing,
        });

        let drained = self.operations.wait_until_empty(deadline).await;
        if !drained {
            self.cancel_in_flight().await;
        }

        *self.state.lock().await = EngineState::Quiesced;
        self.events.send(EngineEvent::StateChanged {
            state: EngineState::Quiesced,
        });
        Ok(())
    }

    async fn cancel_in_flight(&self) {
        let cancelled = self.operations.cancel_all().await;

        for operation_id in cancelled {
            self.events.send(EngineEvent::OperationFinished {
                operation_id,
                terminal: OperationTerminal::Cancelled,
            });
        }
    }

    fn report_lifecycle_result(
        &self,
        action: LifecycleAction,
        result: Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        if let Err(error) = &result {
            self.events.send(EngineEvent::LifecycleFailed {
                action,
                error: error.clone(),
            });
        }
        result
    }
}

fn invalid_state_error() -> EngineError {
    EngineError::new(INVALID_STATE_CODE, EngineErrorCategory::InvalidState, false)
}

fn operation_cancelled_error() -> EngineError {
    EngineError::new(
        OPERATION_CANCELLED_CODE,
        EngineErrorCategory::DeadlineExceeded,
        true,
    )
}

fn terminal_for_result<T>(result: &Result<T, EngineError>) -> OperationTerminal {
    match result {
        Ok(_) => OperationTerminal::Succeeded,
        Err(error) => OperationTerminal::Failed(error.clone()),
    }
}

fn remaining_until(deadline: tokio::time::Instant) -> Duration {
    deadline.saturating_duration_since(tokio::time::Instant::now())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use crate::{
        EngineError, EngineErrorCategory, EngineEvent, EngineState, Operation, OperationResult,
        OperationTerminal, SendTextInput,
    };

    use super::{Engine, EngineRuntime};

    #[derive(Default)]
    struct FakeRuntime {
        execute_calls: AtomicUsize,
        block_operations: AtomicBool,
        operation_started: Notify,
        suspend_calls: AtomicUsize,
        resume_calls: AtomicUsize,
        fail_suspend: AtomicBool,
        fail_resume: AtomicBool,
        shutdown_calls: AtomicUsize,
        fail_shutdown: AtomicBool,
    }

    #[async_trait]
    impl EngineRuntime for FakeRuntime {
        async fn execute(
            &self,
            _operation: Operation,
            cancellation: CancellationToken,
        ) -> Result<OperationResult, EngineError> {
            self.execute_calls.fetch_add(1, Ordering::SeqCst);
            if self.block_operations.load(Ordering::SeqCst) {
                self.operation_started.notify_one();
                cancellation.cancelled().await;
                return Err(EngineError::new(
                    1002,
                    EngineErrorCategory::DeadlineExceeded,
                    true,
                ));
            }
            Ok(OperationResult::Devices(Vec::new()))
        }

        async fn suspend(&self) -> Result<(), EngineError> {
            self.suspend_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_suspend.load(Ordering::SeqCst) {
                return Err(EngineError::new(
                    9002,
                    EngineErrorCategory::Unavailable,
                    true,
                ));
            }
            Ok(())
        }

        async fn resume(&self) -> Result<(), EngineError> {
            self.resume_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_resume.load(Ordering::SeqCst) {
                return Err(EngineError::new(
                    9003,
                    EngineErrorCategory::Unavailable,
                    true,
                ));
            }
            Ok(())
        }

        async fn shutdown(&self, _deadline: Duration) -> Result<(), EngineError> {
            self.shutdown_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_shutdown.load(Ordering::SeqCst) {
                return Err(EngineError::new(9001, EngineErrorCategory::Internal, false));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn execute_is_rejected_after_quiesce_without_reaching_runtime() {
        let runtime = Arc::new(FakeRuntime::default());
        let (engine, _events) = Engine::from_runtime(runtime.clone(), 8);

        engine.quiesce(Duration::from_millis(50)).await.unwrap();
        let error = engine.execute(Operation::ListDevices).await.unwrap_err();

        assert_eq!(error.category(), EngineErrorCategory::InvalidState);
        assert_eq!(runtime.execute_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn quiesce_deadline_cancels_in_flight_operation_with_terminal_event() {
        let runtime = Arc::new(FakeRuntime {
            block_operations: AtomicBool::new(true),
            ..FakeRuntime::default()
        });
        let (engine, mut events) = Engine::from_runtime(runtime.clone(), 8);
        let engine = Arc::new(engine);
        let operation_started = runtime.operation_started.notified();
        let execute_engine = Arc::clone(&engine);
        let execute = tokio::spawn(async move {
            execute_engine
                .execute(Operation::SendText(SendTextInput {
                    text: "private text".into(),
                    target_devices: Vec::new(),
                }))
                .await
        });
        operation_started.await;

        engine.quiesce(Duration::from_millis(10)).await.unwrap();
        let error = tokio::time::timeout(Duration::from_millis(100), execute)
            .await
            .expect("cancelled operation did not finish")
            .expect("execute task panicked")
            .unwrap_err();

        assert_eq!(error.category(), EngineErrorCategory::DeadlineExceeded);
        assert_eq!(
            events.next().await,
            Some(EngineEvent::StateChanged {
                state: EngineState::Quiescing,
            })
        );
        assert!(matches!(
            events.next().await,
            Some(EngineEvent::OperationFinished {
                terminal: OperationTerminal::Cancelled,
                ..
            })
        ));
        assert_eq!(
            events.next().await,
            Some(EngineEvent::StateChanged {
                state: EngineState::Quiesced,
            })
        );
    }

    #[tokio::test]
    async fn suspend_then_resume_keeps_stream_without_retrying_cancelled_work() {
        let runtime = Arc::new(FakeRuntime {
            block_operations: AtomicBool::new(true),
            ..FakeRuntime::default()
        });
        let (engine, mut events) = Engine::from_runtime(runtime.clone(), 8);
        let engine = Arc::new(engine);
        let operation_started = runtime.operation_started.notified();
        let execute_engine = Arc::clone(&engine);
        let execute = tokio::spawn(async move {
            execute_engine
                .execute(Operation::SendText(SendTextInput {
                    text: "private text".into(),
                    target_devices: Vec::new(),
                }))
                .await
        });
        operation_started.await;

        engine.suspend().await.unwrap();
        assert_eq!(engine.lifecycle_state().await, EngineState::Suspended);
        let cancelled = tokio::time::timeout(Duration::from_millis(100), execute)
            .await
            .expect("suspended operation did not finish")
            .unwrap()
            .unwrap_err();
        assert_eq!(cancelled.category(), EngineErrorCategory::DeadlineExceeded);
        assert_eq!(runtime.suspend_calls.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.execute_calls.load(Ordering::SeqCst), 1);

        engine.resume().await.unwrap();
        assert_eq!(engine.lifecycle_state().await, EngineState::Running);
        assert_eq!(runtime.resume_calls.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.execute_calls.load(Ordering::SeqCst), 1);

        let mut states = Vec::new();
        while states.len() < 4 {
            if let Some(EngineEvent::StateChanged { state }) = events.next().await {
                states.push(state);
            }
        }
        assert_eq!(
            states,
            vec![
                EngineState::Quiescing,
                EngineState::Quiesced,
                EngineState::Suspended,
                EngineState::Running,
            ]
        );
    }

    #[tokio::test]
    async fn suspend_failure_is_reported_on_the_engine_event_stream() {
        let runtime = Arc::new(FakeRuntime {
            fail_suspend: AtomicBool::new(true),
            ..FakeRuntime::default()
        });
        let (engine, mut events) = Engine::from_runtime(runtime, 8);

        let error = engine.suspend().await.unwrap_err();

        while let Some(event) = events.next().await {
            if let EngineEvent::LifecycleFailed {
                action,
                error: observed,
            } = event
            {
                assert_eq!(action, crate::LifecycleAction::Suspend);
                assert_eq!(observed, error);
                return;
            }
        }
        panic!("suspend failure was not reported before the event stream closed");
    }

    #[tokio::test]
    async fn resume_failure_is_reported_on_the_engine_event_stream() {
        let runtime = Arc::new(FakeRuntime::default());
        let (engine, mut events) = Engine::from_runtime(runtime.clone(), 8);
        engine.suspend().await.unwrap();
        runtime.fail_resume.store(true, Ordering::SeqCst);

        let error = engine.resume().await.unwrap_err();

        while let Some(event) = events.next().await {
            if let EngineEvent::LifecycleFailed {
                action,
                error: observed,
            } = event
            {
                assert_eq!(action, crate::LifecycleAction::Resume);
                assert_eq!(observed, error);
                return;
            }
        }
        panic!("resume failure was not reported before the event stream closed");
    }

    #[tokio::test]
    async fn shutdown_drains_runtime_and_closes_event_stream() {
        let runtime = Arc::new(FakeRuntime::default());
        let (engine, mut events) = Engine::from_runtime(runtime.clone(), 8);

        engine.shutdown(Duration::from_millis(50)).await.unwrap();
        assert_eq!(engine.lifecycle_state().await, EngineState::Stopped);

        assert_eq!(runtime.shutdown_calls.load(Ordering::SeqCst), 1);
        let states = tokio::time::timeout(Duration::from_millis(100), async {
            let mut states = Vec::new();
            while let Some(event) = events.next().await {
                if let EngineEvent::StateChanged { state } = event {
                    states.push(state);
                }
            }
            states
        })
        .await
        .expect("event stream remained open after shutdown");
        assert_eq!(
            states,
            vec![
                EngineState::Quiescing,
                EngineState::Quiesced,
                EngineState::ShuttingDown,
                EngineState::Stopped,
            ]
        );
        let error = engine.execute(Operation::ListDevices).await.unwrap_err();
        assert_eq!(error.category(), EngineErrorCategory::InvalidState);
    }

    #[tokio::test]
    async fn shutdown_failure_is_reported_before_stream_closes() {
        let runtime = Arc::new(FakeRuntime {
            fail_shutdown: AtomicBool::new(true),
            ..FakeRuntime::default()
        });
        let (engine, mut events) = Engine::from_runtime(runtime, 8);

        let error = engine
            .shutdown(Duration::from_millis(50))
            .await
            .unwrap_err();
        assert_eq!(error.category(), EngineErrorCategory::Internal);

        let mut fatal = None;
        while let Some(event) = events.next().await {
            if let EngineEvent::Fatal { error } = event {
                fatal = Some(error);
            }
        }
        assert_eq!(fatal, Some(error));
    }
}
