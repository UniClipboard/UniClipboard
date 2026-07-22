use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use napi::bindgen_prelude::{Buffer, FromNapiValue};
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, Status};
use uc_engine::{
    HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory, HostClipboard,
    HostClipboardSnapshot, HostDirectories, HostFileAccess, HostFileHandle, HostFileMetadata,
    HostSecureStorage,
};

use crate::OhHost;

const HOST_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn unref_callbacks(host: &mut OhHost, env: &Env) -> napi::Result<()> {
    host.secure_storage_get.unref(env)?;
    host.secure_storage_set.unref(env)?;
    host.secure_storage_delete.unref(env)
}

pub(crate) fn capabilities(host: OhHost) -> napi::Result<HostCapabilities> {
    let directories = HostDirectories::new(
        PathBuf::from(host.private_data_directory),
        PathBuf::from(host.cache_directory),
        PathBuf::from(host.temporary_directory),
    );
    for directory in [
        directories.private_data(),
        directories.cache(),
        directories.temporary(),
    ] {
        std::fs::create_dir_all(directory).map_err(|_| host_error("create host directory"))?;
    }
    Ok(HostCapabilities::new(
        directories,
        Box::new(OhSecureStorage {
            get: host.secure_storage_get,
            set: host.secure_storage_set,
            delete: host.secure_storage_delete,
        }),
        Box::new(UnavailableClipboard),
        Box::new(UnavailableFiles),
    ))
}

fn host_error(operation: &str) -> napi::Error {
    napi::Error::new(Status::GenericFailure, format!("OHOS_HOST_IO:{operation}"))
}

fn call_host<T, D>(
    callback: &ThreadsafeFunction<T, ErrorStrategy::Fatal>,
    value: T,
) -> Result<D, HostCapabilityError>
where
    T: 'static,
    D: FromNapiValue + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let status = callback.call_with_return_value(
        value,
        ThreadsafeFunctionCallMode::NonBlocking,
        move |returned: D| {
            let _ = sender.send(returned);
            Ok(())
        },
    );
    if status != Status::Ok {
        return Err(callback_error());
    }
    receiver
        .recv_timeout(HOST_CALLBACK_TIMEOUT)
        .map_err(|_| callback_error())
}

fn callback_error() -> HostCapabilityError {
    HostCapabilityError::new(
        HostCapabilityErrorCategory::Unavailable,
        "HarmonyOS host callback unavailable",
    )
}

struct OhSecureStorage {
    get: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
    set: ThreadsafeFunction<(String, Buffer), ErrorStrategy::Fatal>,
    delete: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
}

impl HostSecureStorage for OhSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        call_host::<_, Option<Buffer>>(&self.get, key.to_owned())
            .map(|value| value.map(|bytes| bytes.to_vec()))
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        call_host::<_, ()>(&self.set, (key.to_owned(), Buffer::from(value.to_vec())))
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        call_host::<_, ()>(&self.delete, key.to_owned())
    }
}

struct UnavailableClipboard;

impl HostClipboard for UnavailableClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        Err(unavailable_capability("clipboard read"))
    }

    fn write(&self, _snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability("clipboard write"))
    }
}

struct UnavailableFiles;

impl HostFileAccess for UnavailableFiles {
    fn metadata(&self, _handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        Err(unavailable_capability("file metadata"))
    }

    fn read_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        Err(unavailable_capability("file read"))
    }

    fn write_chunk(
        &self,
        _handle: &HostFileHandle,
        _offset: u64,
        _bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability("file write"))
    }

    fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        Err(unavailable_capability("file finish"))
    }
}

fn unavailable_capability(operation: &str) -> HostCapabilityError {
    HostCapabilityError::new(
        HostCapabilityErrorCategory::Unavailable,
        format!("HarmonyOS {operation} is not installed"),
    )
}
