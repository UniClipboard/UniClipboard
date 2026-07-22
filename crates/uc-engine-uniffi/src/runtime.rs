use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use uc_engine::{
    CreateSpaceInput, Engine, EngineConfig, HostCapabilities, HostCapabilityError,
    HostCapabilityErrorCategory, HostClipboard, HostClipboardSnapshot, HostDirectories,
    HostFileAccess, HostFileHandle, HostFileMetadata, HostSecureStorage, Operation,
    OperationResult, SecretString,
};
use zeroize::Zeroizing;

use crate::{BindingConfig, BindingError, BindingHost, HostBindingError};

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SpaceCreated {
    pub space_id: String,
    pub self_device_id: String,
    pub identity_fingerprint: String,
}

enum WorkerCommand {
    CreateSpace {
        device_name: Option<String>,
        passphrase: Zeroizing<String>,
        response: mpsc::Sender<Result<SpaceCreated, BindingError>>,
    },
    Shutdown {
        deadline: Duration,
        response: mpsc::Sender<Result<(), BindingError>>,
    },
}

#[derive(uniffi::Object)]
pub struct MobileEngine {
    commands: Mutex<Option<tokio::sync::mpsc::UnboundedSender<WorkerCommand>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[uniffi::export]
impl MobileEngine {
    #[uniffi::constructor]
    pub fn start(
        config: BindingConfig,
        host: Arc<dyn BindingHost>,
    ) -> Result<Arc<Self>, BindingError> {
        let capabilities = host_capabilities(Arc::clone(&host))?;
        let config = EngineConfig::new(config.app_version).with_profile_id(config.profile_id);
        let (commands, requests) = tokio::sync::mpsc::unbounded_channel();
        let (started, start_result) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("uc-engine-uniffi".to_owned())
            .spawn(move || run_worker(config, capabilities, requests, started))
            .map_err(|_| BindingError::RuntimeUnavailable)?;

        match start_result.recv() {
            Ok(Ok(())) => Ok(Arc::new(Self {
                commands: Mutex::new(Some(commands)),
                worker: Mutex::new(Some(worker)),
            })),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(BindingError::RuntimeUnavailable)
            }
        }
    }

    pub fn create_space(
        &self,
        device_name: Option<String>,
        passphrase: String,
    ) -> Result<SpaceCreated, BindingError> {
        let passphrase = Zeroizing::new(passphrase);
        let commands = self.command_sender()?;
        let (response, result) = mpsc::channel();
        commands
            .send(WorkerCommand::CreateSpace {
                device_name,
                passphrase,
                response,
            })
            .map_err(|_| BindingError::RuntimeUnavailable)?;
        result
            .recv()
            .map_err(|_| BindingError::RuntimeUnavailable)?
    }

    pub fn shutdown(&self, deadline_ms: u64) -> Result<(), BindingError> {
        self.shutdown_inner(Duration::from_millis(deadline_ms), true)
    }
}

impl MobileEngine {
    fn command_sender(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedSender<WorkerCommand>, BindingError> {
        lock(&self.commands)
            .as_ref()
            .cloned()
            .ok_or(BindingError::AlreadyStopped)
    }

    fn shutdown_inner(&self, deadline: Duration, join: bool) -> Result<(), BindingError> {
        let commands = lock(&self.commands)
            .take()
            .ok_or(BindingError::AlreadyStopped)?;
        let (response, result) = mpsc::channel();
        let shutdown_result = commands
            .send(WorkerCommand::Shutdown { deadline, response })
            .map_err(|_| BindingError::RuntimeUnavailable)
            .and_then(|()| {
                result
                    .recv()
                    .map_err(|_| BindingError::RuntimeUnavailable)?
            });
        let join_result = if join { self.join_worker() } else { Ok(()) };
        shutdown_result.and(join_result)
    }

    fn join_worker(&self) -> Result<(), BindingError> {
        if let Some(worker) = lock(&self.worker).take() {
            worker
                .join()
                .map_err(|_| BindingError::RuntimeUnavailable)?;
        }
        Ok(())
    }
}

impl Drop for MobileEngine {
    fn drop(&mut self) {
        let _ = self.shutdown_inner(Duration::ZERO, true);
    }
}

fn run_worker(
    config: EngineConfig,
    host: HostCapabilities,
    requests: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    started: mpsc::Sender<Result<(), BindingError>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let _ = started.send(Err(BindingError::RuntimeUnavailable));
            return;
        }
    };
    runtime.block_on(run_worker_loop(config, host, requests, started));
}

async fn run_worker_loop(
    config: EngineConfig,
    host: HostCapabilities,
    mut requests: tokio::sync::mpsc::UnboundedReceiver<WorkerCommand>,
    started: mpsc::Sender<Result<(), BindingError>>,
) {
    let (engine, _events) = match Engine::start(config, host).await {
        Ok(started_engine) => started_engine,
        Err(error) => {
            let _ = started.send(Err(error.into()));
            return;
        }
    };
    if started.send(Ok(())).is_err() {
        let _ = engine.shutdown(Duration::ZERO).await;
        return;
    }

    while let Some(command) = requests.recv().await {
        match command {
            WorkerCommand::CreateSpace {
                device_name,
                passphrase,
                response,
            } => {
                let result = engine
                    .execute(Operation::CreateSpace(CreateSpaceInput {
                        device_name,
                        passphrase: SecretString::new(passphrase.as_str()),
                        passphrase_confirmation: SecretString::new(passphrase.as_str()),
                    }))
                    .await
                    .map_err(BindingError::from)
                    .and_then(map_space_created);
                let _ = response.send(result);
            }
            WorkerCommand::Shutdown { deadline, response } => {
                let result = engine.shutdown(deadline).await.map_err(BindingError::from);
                let _ = response.send(result);
                break;
            }
        }
    }
}

fn map_space_created(result: OperationResult) -> Result<SpaceCreated, BindingError> {
    match result {
        OperationResult::SpaceCreated {
            space_id,
            self_device_id,
            identity_fingerprint,
        } => Ok(SpaceCreated {
            space_id,
            self_device_id,
            identity_fingerprint,
        }),
        _ => Err(BindingError::UnexpectedResult),
    }
}

fn host_capabilities(host: Arc<dyn BindingHost>) -> Result<HostCapabilities, BindingError> {
    let directories = HostDirectories::new(
        host_path(host.private_data_directory())?,
        host_path(host.cache_directory())?,
        host_path(host.temporary_directory())?,
    );
    for directory in [
        directories.private_data(),
        directories.cache(),
        directories.temporary(),
    ] {
        std::fs::create_dir_all(directory).map_err(|_| BindingError::HostIo)?;
    }
    Ok(HostCapabilities::new(
        directories,
        Box::new(BindingSecureStorage {
            host: Arc::clone(&host),
        }),
        Box::new(UnavailableClipboard),
        Box::new(UnavailableFiles),
    ))
}

fn host_path(result: Result<String, HostBindingError>) -> Result<PathBuf, BindingError> {
    result.map(PathBuf::from).map_err(map_binding_host_error)
}

fn map_binding_host_error(error: HostBindingError) -> BindingError {
    match error {
        HostBindingError::Unavailable => BindingError::HostUnavailable,
        HostBindingError::PermissionDenied => BindingError::HostPermissionDenied,
        HostBindingError::InvalidHandle => BindingError::HostInvalidHandle,
        HostBindingError::Io => BindingError::HostIo,
    }
}

fn map_host_capability_error(error: HostBindingError) -> HostCapabilityError {
    let category = match error {
        HostBindingError::Unavailable => HostCapabilityErrorCategory::Unavailable,
        HostBindingError::PermissionDenied => HostCapabilityErrorCategory::PermissionDenied,
        HostBindingError::InvalidHandle => HostCapabilityErrorCategory::InvalidHandle,
        HostBindingError::Io => HostCapabilityErrorCategory::Io,
    };
    HostCapabilityError::new(category, "binding host callback failed")
}

struct BindingSecureStorage {
    host: Arc<dyn BindingHost>,
}

impl HostSecureStorage for BindingSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        self.host
            .secure_storage_get(key.to_owned())
            .map_err(map_host_capability_error)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.host
            .secure_storage_set(key.to_owned(), value.to_vec())
            .map_err(map_host_capability_error)
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.host
            .secure_storage_delete(key.to_owned())
            .map_err(map_host_capability_error)
    }
}

struct UnavailableClipboard;

impl HostClipboard for UnavailableClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Err(unavailable_capability())
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability())
    }
}

struct UnavailableFiles;

impl HostFileAccess for UnavailableFiles {
    fn metadata(&self, _handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        Err(unavailable_capability())
    }

    fn read_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        Err(unavailable_capability())
    }

    fn write_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability())
    }

    fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability())
    }
}

fn unavailable_capability() -> HostCapabilityError {
    HostCapabilityError::new(
        HostCapabilityErrorCategory::Unavailable,
        "capability is not exposed by this binding slice",
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}
