use std::io::Read;
use std::path::Path;

use crate::{HostCapabilityError, HostFileAccess, HostFileHandle};

const COPY_CHUNK_SIZE: usize = 64 * 1024;

pub(crate) enum HostFileCopyError {
    SourceIo,
    Host(HostCapabilityError),
}

pub(crate) async fn copy_path_to_host(
    files: &dyn HostFileAccess,
    destination: &HostFileHandle,
    source: &Path,
) -> Result<(), HostFileCopyError> {
    let mut input = std::fs::File::open(source).map_err(|_| HostFileCopyError::SourceIo)?;
    let mut buffer = vec![0_u8; COPY_CHUNK_SIZE];
    let mut offset = 0_u64;
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|_| HostFileCopyError::SourceIo)?;
        if read == 0 {
            break;
        }
        files
            .write_chunk(destination, offset, &buffer[..read])
            .map_err(HostFileCopyError::Host)?;
        offset += read as u64;
        tokio::task::yield_now().await;
    }
    files
        .finish_write(destination)
        .map_err(HostFileCopyError::Host)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use uc_core::ids::RepresentationId;

    use super::*;
    use crate::{HostCapabilityError, HostFileMetadata};

    #[derive(Default)]
    struct RecordingFiles {
        bytes: Mutex<Vec<u8>>,
        finished: AtomicBool,
    }

    impl HostFileAccess for RecordingFiles {
        fn metadata(
            &self,
            _handle: &HostFileHandle,
        ) -> Result<HostFileMetadata, HostCapabilityError> {
            unreachable!("copy does not query destination metadata")
        }

        fn read_chunk(
            &self,
            _handle: &HostFileHandle,
            _offset: u64,
            _max_bytes: u32,
        ) -> Result<Vec<u8>, HostCapabilityError> {
            unreachable!("copy does not read from destination")
        }

        fn write_chunk(
            &self,
            _handle: &HostFileHandle,
            offset: u64,
            bytes: &[u8],
        ) -> Result<(), HostCapabilityError> {
            let mut output = self.bytes.lock().expect("output lock");
            assert_eq!(offset, output.len() as u64);
            output.extend_from_slice(bytes);
            Ok(())
        }

        fn finish_write(&self, _handle: &HostFileHandle) -> Result<(), HostCapabilityError> {
            self.finished.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn copy_writes_every_chunk_and_finishes_the_host_handle() {
        let source =
            std::env::temp_dir().join(format!("uc-engine-host-copy-{}", RepresentationId::new()));
        let expected = vec![0x5a; COPY_CHUNK_SIZE * 2 + 17];
        std::fs::write(&source, &expected).expect("write source");
        let files = RecordingFiles::default();

        let result = copy_path_to_host(&files, &HostFileHandle::new("destination"), &source).await;
        let _ = std::fs::remove_file(source);

        assert!(result.is_ok());
        assert_eq!(*files.bytes.lock().expect("output lock"), expected);
        assert!(files.finished.load(Ordering::SeqCst));
    }
}
