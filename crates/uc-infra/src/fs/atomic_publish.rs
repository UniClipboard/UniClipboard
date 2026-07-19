//! Implementation of [`AtomicPublishPort`] over the platforms' native
//! no-replace rename primitives.
//!
//! `std::fs::rename` is the wrong tool here: POSIX `rename(2)` silently
//! replaces an existing destination, and on Windows `MOVEFILE_REPLACE_EXISTING`
//! is opt-in but the API still needs the "no replace" case spelled out. Each
//! platform has a primitive that fails instead of clobbering:
//!
//! - Linux: `renameat2(RENAME_NOREPLACE)`
//! - macOS: `renamex_np(RENAME_EXCL)`
//! - Windows: `MoveFileExW` with neither `MOVEFILE_REPLACE_EXISTING` nor
//!   `MOVEFILE_COPY_ALLOWED`
//!
//! Support is filesystem-dependent, not just platform-dependent: network
//! mounts, exFAT, some FUSE and overlay filesystems reject the flag. That is
//! what [`FsAtomicPublisher::supports_no_replace`] is for.

use std::path::Path;

use async_trait::async_trait;
use tracing::{debug, warn};
use uc_core::ports::atomic_publish::{AtomicPublishPort, PublishError};

pub struct FsAtomicPublisher;

impl FsAtomicPublisher {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self)
    }
}

impl Default for FsAtomicPublisher {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl AtomicPublishPort for FsAtomicPublisher {
    async fn publish_no_replace(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError> {
        let source = source.to_path_buf();
        let destination = destination.to_path_buf();
        tokio::task::spawn_blocking(move || rename_no_replace(&source, &destination))
            .await
            .map_err(|err| PublishError::Io(format!("publish task did not run: {err}")))?
    }

    async fn publish_into_free_name(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError> {
        let source = source.to_path_buf();
        let destination = destination.to_path_buf();
        tokio::task::spawn_blocking(move || rename_plain(&source, &destination))
            .await
            .map_err(|err| PublishError::Io(format!("publish task did not run: {err}")))?
    }

    async fn supports_no_replace(&self, probe_dir: &Path) -> bool {
        let probe_dir = probe_dir.to_path_buf();
        match tokio::task::spawn_blocking(move || probe_no_replace(&probe_dir)).await {
            Ok(supported) => supported,
            Err(err) => {
                warn!(error = %err, "no-replace probe task did not run; assuming unsupported");
                false
            }
        }
    }
}

/// Confirm the volume behind `probe_dir` really refuses to replace, by doing
/// the thing rather than trusting the platform.
///
/// A filesystem can accept the flag and ignore it, which would be worse than
/// rejecting it outright — the caller would believe it holds a guarantee it
/// does not have. So the probe asserts the *refusal*: only an explicit
/// `DestinationExists` counts as support. Both probe entries are directories,
/// matching what publication actually moves.
fn probe_no_replace(probe_dir: &Path) -> bool {
    // Unique per probe: a shared name would let two concurrent probes in one
    // directory tear down each other's entries and report a false negative.
    let token = uuid::Uuid::new_v4();
    let source = probe_dir.join(format!(".uniclip-probe-{token}-src"));
    let occupied = probe_dir.join(format!(".uniclip-probe-{token}-dst"));

    let mut created = Vec::new();
    let result = (|| {
        std::fs::create_dir(&source)?;
        created.push(source.clone());
        std::fs::create_dir(&occupied)?;
        created.push(occupied.clone());
        Ok::<_, std::io::Error>(rename_no_replace(&source, &occupied))
    })();

    for path in &created {
        if let Err(err) = std::fs::remove_dir_all(path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(error = %err, "failed to clean up a no-replace probe entry");
            }
        }
    }

    match result {
        Ok(Err(PublishError::DestinationExists)) => true,
        Ok(Err(PublishError::Unsupported)) => {
            debug!("volume rejects no-replace publication");
            false
        }
        // The rename reported success against an occupied destination: the
        // flag was accepted and disregarded, so the guarantee is a fiction here.
        Ok(Ok(())) => {
            warn!(
                "volume accepts the no-replace flag but replaces anyway; treating as unsupported"
            );
            false
        }
        Ok(Err(PublishError::Io(detail))) => {
            debug!(detail, "no-replace probe failed; assuming unsupported");
            false
        }
        Err(err) => {
            debug!(error = %err, "could not stage a no-replace probe; assuming unsupported");
            false
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), PublishError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let to_c = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| PublishError::Io("path contains an interior NUL".to_string()))
    };
    let source = to_c(source)?;
    let destination = to_c(destination)?;

    let rc = unsafe { rename_excl_syscall(source.as_ptr(), destination.as_ptr()) };
    if rc == 0 {
        return Ok(());
    }
    Err(classify_os_error(std::io::Error::last_os_error()))
}

/// Mobile Unix targets currently have no verified native no-replace primitive.
/// Refuse publication rather than emulating errno or silently replacing data.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn rename_no_replace(_source: &Path, _destination: &Path) -> Result<(), PublishError> {
    Err(PublishError::Unsupported)
}

#[cfg(target_os = "linux")]
unsafe fn rename_excl_syscall(
    source: *const libc::c_char,
    destination: *const libc::c_char,
) -> libc::c_int {
    // Called through `syscall` rather than the glibc `renameat2` wrapper: musl
    // has no wrapper, and the raw syscall behaves identically on both.
    unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source,
            libc::AT_FDCWD,
            destination,
            libc::RENAME_NOREPLACE,
        ) as libc::c_int
    }
}

#[cfg(target_os = "macos")]
unsafe fn rename_excl_syscall(
    source: *const libc::c_char,
    destination: *const libc::c_char,
) -> libc::c_int {
    unsafe { libc::renamex_np(source, destination, libc::RENAME_EXCL) }
}

#[cfg(unix)]
fn classify_os_error(err: std::io::Error) -> PublishError {
    match err.raw_os_error() {
        Some(libc::EEXIST) | Some(libc::ENOTEMPTY) => PublishError::DestinationExists,
        // ENOSYS: kernel too old. EINVAL: the flag reached a filesystem that
        // does not implement it. ENOTSUP: macOS's answer for the same.
        // EXDEV: the two paths are on different volumes, so no rename can be
        // atomic regardless of flags.
        Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::ENOTSUP) | Some(libc::EXDEV) => {
            PublishError::Unsupported
        }
        _ => PublishError::Io(describe_io(&err)),
    }
}

/// Move without asking the platform for the no-replace guarantee.
///
/// `std::fs::rename` is the right tool once the guarantee is off the table:
/// it reaches volumes the no-replace primitives reject outright — exFAT, some
/// FUSE and network mounts — which is the entire reason this variant exists.
/// The atomicity of the rename itself is unaffected.
fn rename_plain(source: &Path, destination: &Path) -> Result<(), PublishError> {
    std::fs::rename(source, destination).map_err(classify_os_error)
}

#[cfg(windows)]
fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), PublishError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let to_wide = |path: &Path| {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(PublishError::Io(
                "path contains an interior NUL".to_string(),
            ));
        }
        wide.push(0);
        Ok(wide)
    };
    let source = to_wide(source)?;
    let destination = to_wide(destination)?;

    // Flags deliberately empty. MOVEFILE_REPLACE_EXISTING would clobber the
    // destination; MOVEFILE_COPY_ALLOWED would turn a cross-volume move into a
    // non-atomic copy. Without either, an occupied destination fails and a
    // cross-volume move fails, which is exactly the contract.
    let ok = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) };
    if ok != 0 {
        return Ok(());
    }
    Err(classify_os_error(std::io::Error::last_os_error()))
}

#[cfg(windows)]
fn classify_os_error(err: std::io::Error) -> PublishError {
    use windows_sys::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_NOT_SAME_DEVICE,
    };

    let code = err.raw_os_error().unwrap_or_default() as u32;
    match code {
        ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS => PublishError::DestinationExists,
        ERROR_NOT_SAME_DEVICE => PublishError::Unsupported,
        _ => PublishError::Io(describe_io(&err)),
    }
}

/// Describe a failure by kind and code only.
///
/// `io::Error`'s own `Display` interpolates the path the OS reported, and
/// these strings travel into places that must stay free of user content.
fn describe_io(err: &std::io::Error) -> String {
    match err.raw_os_error() {
        Some(code) => format!("{:?} (os error {code})", err.kind()),
        None => format!("{:?}", err.kind()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn publisher() -> FsAtomicPublisher {
        FsAtomicPublisher
    }

    #[tokio::test]
    async fn publishes_directory_onto_a_free_name() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("staged");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("a.txt"), b"hi").unwrap();
        let destination = tmp.path().join("final");

        publisher()
            .publish_no_replace(&source, &destination)
            .await
            .expect("published");

        assert!(!source.exists());
        assert_eq!(std::fs::read(destination.join("a.txt")).unwrap(), b"hi");
    }

    #[tokio::test]
    async fn refuses_an_occupied_name_and_leaves_both_sides_intact() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("staged");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("new.txt"), b"new").unwrap();
        let destination = tmp.path().join("final");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("existing.txt"), b"existing").unwrap();

        let err = publisher()
            .publish_no_replace(&source, &destination)
            .await
            .expect_err("must refuse");

        assert_eq!(err, PublishError::DestinationExists);
        // The whole point: the user's existing content survives untouched, and
        // nothing merged into it.
        assert_eq!(
            std::fs::read(destination.join("existing.txt")).unwrap(),
            b"existing"
        );
        assert!(!destination.join("new.txt").exists());
        assert!(source.join("new.txt").exists());
    }

    #[tokio::test]
    async fn refuses_an_occupied_name_held_by_a_file() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("staged");
        std::fs::create_dir(&source).unwrap();
        let destination = tmp.path().join("final");
        std::fs::write(&destination, b"a file holds this name").unwrap();

        let err = publisher()
            .publish_no_replace(&source, &destination)
            .await
            .expect_err("must refuse");

        assert_eq!(err, PublishError::DestinationExists);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"a file holds this name"
        );
    }

    #[tokio::test]
    async fn publishes_a_file_root_onto_a_free_name() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("staged.txt");
        std::fs::write(&source, b"payload").unwrap();
        let destination = tmp.path().join("final.txt");

        publisher()
            .publish_no_replace(&source, &destination)
            .await
            .expect("published");

        assert_eq!(std::fs::read(&destination).unwrap(), b"payload");
    }

    #[tokio::test]
    async fn reports_unsupported_for_a_missing_parent_rather_than_pretending() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("staged");
        std::fs::create_dir(&source).unwrap();
        let destination = tmp.path().join("no-such-dir").join("final");

        let err = publisher()
            .publish_no_replace(&source, &destination)
            .await
            .expect_err("must fail");

        assert!(matches!(err, PublishError::Io(_)), "unexpected: {err:?}");
    }

    #[tokio::test]
    async fn probe_reports_support_on_a_local_temp_volume_and_cleans_up() {
        let tmp = TempDir::new().unwrap();

        assert!(publisher().supports_no_replace(tmp.path()).await);

        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(leftovers.is_empty(), "probe left {leftovers:?} behind");
    }

    #[tokio::test]
    async fn probe_reports_unsupported_when_the_directory_is_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(
            !publisher()
                .supports_no_replace(&tmp.path().join("absent"))
                .await
        );
    }

    #[test]
    fn io_description_omits_the_path_the_os_reported() {
        let err = std::fs::metadata("/definitely/not/here/secret-folder-name").unwrap_err();
        let described = describe_io(&err);
        assert!(!described.contains("secret-folder-name"), "{described}");
    }
}
