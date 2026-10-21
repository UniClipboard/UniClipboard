//! Process-wide allocator policy for the long-lived desktop daemon.

use anyhow::Result;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
use libc::{mallopt, M_ARENA_MAX, M_MMAP_THRESHOLD};

/// Bound arena retention and return large transient buffers through mmap.
/// Explicit glibc settings remain authoritative for deployments and profiling.
pub fn configure() -> Result<()> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let tunables = std::env::var("GLIBC_TUNABLES").unwrap_or_default();
        let policy = [
            (M_ARENA_MAX, 2, "MALLOC_ARENA_MAX", "glibc.malloc.arena_max"),
            (
                M_MMAP_THRESHOLD,
                128 * 1024,
                "MALLOC_MMAP_THRESHOLD_",
                "glibc.malloc.mmap_threshold",
            ),
        ];
        for (parameter, value, environment, tunable) in policy {
            let overridden = std::env::var_os(environment).is_some()
                || tunables.split(':').any(|setting| {
                    setting
                        .split_once('=')
                        .is_some_and(|(name, _)| name == tunable)
                });
            if overridden {
                continue;
            }

            // SAFETY: mallopt accepts these positive values without pointer arguments.
            // Configure the process before spawning daemon worker threads.
            let configured = unsafe { mallopt(parameter, value) };
            anyhow::ensure!(
                configured != 0,
                "failed to configure daemon allocator parameter {parameter}"
            );
        }
    }
    Ok(())
}
