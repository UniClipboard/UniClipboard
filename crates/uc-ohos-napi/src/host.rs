use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use napi::bindgen_prelude::{Buffer, FromNapiValue};
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi::{Env, JsObject, Status};
use uc_engine::{
    HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory, HostClipboard,
    HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories, HostFileAccess,
    HostFileHandle, HostFileMetadata, HostSecureStorage,
};

use crate::{OhClipboardRepresentation, OhClipboardSnapshot, OhHost};

const HOST_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn unref_callbacks(host: &mut OhHost, env: &Env) -> napi::Result<()> {
    host.secure_storage_get.unref(env)?;
    host.secure_storage_set.unref(env)?;
    host.secure_storage_delete.unref(env)?;
    host.file_metadata.unref(env)?;
    host.file_read_chunk.unref(env)?;
    host.file_write_chunk.unref(env)?;
    host.file_finish_write.unref(env)?;
    host.clipboard_read.unref(env)?;
    host.clipboard_write.unref(env)
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
        Box::new(OhClipboard {
            read: host.clipboard_read,
            write: host.clipboard_write,
        }),
        Box::new(OhFiles {
            metadata: host.file_metadata,
            read_chunk: host.file_read_chunk,
            write_chunk: host.file_write_chunk,
            finish_write: host.file_finish_write,
        }),
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
    call_host_with(callback, value, Ok)
}

fn call_host_with<T, J, D, F>(
    callback: &ThreadsafeFunction<T, ErrorStrategy::Fatal>,
    value: T,
    convert: F,
) -> Result<D, HostCapabilityError>
where
    T: 'static,
    J: FromNapiValue + 'static,
    D: Send + 'static,
    F: FnOnce(J) -> Result<D, HostCapabilityError> + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    let status = callback.call_with_return_value::<J, _>(
        value,
        ThreadsafeFunctionCallMode::NonBlocking,
        move |returned| {
            let _ = sender.send(convert(returned));
            Ok(())
        },
    );
    if status != Status::Ok {
        return Err(callback_error());
    }
    receiver
        .recv_timeout(HOST_CALLBACK_TIMEOUT)
        .map_err(|_| callback_error())?
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

struct OhClipboard {
    read: ThreadsafeFunction<(), ErrorStrategy::Fatal>,
    write: ThreadsafeFunction<OhClipboardSnapshot, ErrorStrategy::Fatal>,
}

impl HostClipboard for OhClipboard {
    fn read(&self) -> Result<HostClipboardSnapshot, HostCapabilityError> {
        call_host_with::<_, JsObject, _, _>(&self.read, (), clipboard_snapshot_from_js)
    }

    fn write(&self, snapshot: HostClipboardSnapshot) -> Result<(), HostCapabilityError> {
        call_host(
            &self.write,
            OhClipboardSnapshot {
                observed_at_ms: snapshot.observed_at_ms as f64,
                representations: snapshot
                    .representations
                    .into_iter()
                    .map(clipboard_representation_to_host)
                    .collect(),
            },
        )
    }
}

fn clipboard_snapshot_from_js(
    snapshot: JsObject,
) -> Result<HostClipboardSnapshot, HostCapabilityError> {
    let observed_at_ms = property::<f64>(&snapshot, "observedAtMs")?;
    let representations = property::<Vec<JsObject>>(&snapshot, "representations")?;
    Ok(HostClipboardSnapshot {
        observed_at_ms: parse_safe_i64(observed_at_ms)?,
        representations: representations
            .into_iter()
            .map(clipboard_representation_from_js)
            .collect::<Result<_, _>>()?,
    })
}

fn clipboard_representation_from_js(
    representation: JsObject,
) -> Result<HostClipboardRepresentation, HostCapabilityError> {
    let kind = property::<String>(&representation, "kind")?;
    let format = property::<String>(&representation, "format")?;
    let mime_type = property::<Option<String>>(&representation, "mimeType")?;
    match kind.as_str() {
        "inline" => Ok(HostClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes: property::<Buffer>(&representation, "bytes")?.to_vec(),
        }),
        "file" => Ok(HostClipboardRepresentation::File {
            format,
            handle: HostFileHandle::new(property::<String>(&representation, "handle")?),
            display_name: property::<String>(&representation, "displayName")?,
            mime_type,
            size_bytes: parse_u64(&property::<String>(&representation, "sizeBytes")?)?,
        }),
        _ => Err(host_contract_error()),
    }
}

fn clipboard_representation_to_host(
    representation: HostClipboardRepresentation,
) -> OhClipboardRepresentation {
    match representation {
        HostClipboardRepresentation::Inline {
            format,
            mime_type,
            bytes,
        } => OhClipboardRepresentation {
            kind: "inline".to_owned(),
            format,
            mime_type,
            bytes: Some(Buffer::from(bytes)),
            handle: None,
            display_name: None,
            size_bytes: None,
        },
        HostClipboardRepresentation::File {
            format,
            handle,
            display_name,
            mime_type,
            size_bytes,
        } => OhClipboardRepresentation {
            kind: "file".to_owned(),
            format,
            mime_type,
            bytes: None,
            handle: Some(handle.as_str().to_owned()),
            display_name: Some(display_name),
            size_bytes: Some(size_bytes.to_string()),
        },
    }
}

fn parse_safe_i64(value: f64) -> Result<i64, HostCapabilityError> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    if value.is_finite()
        && value.fract() == 0.0
        && (-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value)
    {
        return Ok(value as i64);
    }
    Err(host_contract_error())
}

fn host_contract_error() -> HostCapabilityError {
    HostCapabilityError::new(
        HostCapabilityErrorCategory::Io,
        "HarmonyOS host returned an invalid capability value",
    )
}

fn property<T>(object: &JsObject, name: &str) -> Result<T, HostCapabilityError>
where
    T: FromNapiValue + napi::bindgen_prelude::ValidateNapiValue,
{
    object
        .get_named_property(name)
        .map_err(|_| host_contract_error())
}

struct OhFiles {
    metadata: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
    read_chunk: ThreadsafeFunction<(String, String, u32), ErrorStrategy::Fatal>,
    write_chunk: ThreadsafeFunction<(String, String, Buffer), ErrorStrategy::Fatal>,
    finish_write: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
}

impl HostFileAccess for OhFiles {
    fn metadata(&self, handle: &HostFileHandle) -> Result<HostFileMetadata, HostCapabilityError> {
        call_host_with::<_, JsObject, _, _>(
            &self.metadata,
            handle.as_str().to_owned(),
            file_metadata_from_js,
        )
    }

    fn read_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostCapabilityError> {
        call_host::<_, Buffer>(
            &self.read_chunk,
            (handle.as_str().to_owned(), offset.to_string(), max_bytes),
        )
        .map(|bytes| bytes.to_vec())
    }

    fn write_chunk(
        &self,
        handle: &HostFileHandle,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), HostCapabilityError> {
        call_host(
            &self.write_chunk,
            (
                handle.as_str().to_owned(),
                offset.to_string(),
                Buffer::from(bytes.to_vec()),
            ),
        )
    }

    fn finish_write(&self, handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
        call_host(&self.finish_write, handle.as_str().to_owned())
    }
}

fn file_metadata_from_js(metadata: JsObject) -> Result<HostFileMetadata, HostCapabilityError> {
    Ok(HostFileMetadata {
        display_name: property(&metadata, "displayName")?,
        size_bytes: parse_u64(&property::<String>(&metadata, "sizeBytes")?)?,
        mime_type: property(&metadata, "mimeType")?,
    })
}

fn parse_u64(value: &str) -> Result<u64, HostCapabilityError> {
    value.parse().map_err(|_| {
        HostCapabilityError::new(
            HostCapabilityErrorCategory::InvalidHandle,
            "HarmonyOS host returned an invalid file size or offset",
        )
    })
}
