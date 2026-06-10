//! Win32-native process liveness and termination primitives.
//!
//! Replaces the previous `taskkill`/`tasklist` shell-outs, which had two
//! field-confirmed failure modes:
//!
//! 1. **Console-window flashes.** The GUI shell is a `windows_subsystem =
//!    "windows"` process with no console; every console-tool child it spawned
//!    popped a fresh console window. `terminate_and_wait` polled `tasklist`
//!    every 100ms, so a 10s timeout flashed ~100 black boxes in a row.
//! 2. **Locale-dependent parsing.** Liveness was inferred from tasklist's
//!    no-match banner ("INFO: ..."), which on zh-CN Windows prints
//!    "信息: ..." — a dead PID read as alive forever, timing out every
//!    update install with "daemon still running".
//!
//! Win32 calls have neither problem: no child process (nothing to flash) and
//! no text output (nothing to parse). `WaitForSingleObject` on the process
//! handle also waits on the *kernel object*, which is strictly more precise
//! than polling a process-table snapshot.

use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

/// Owned process handle: closes on drop so early returns can't leak it.
struct ProcessHandle(HANDLE);

impl ProcessHandle {
    /// Open `pid` with `access`. `None` when the PID does not exist (or is
    /// inaccessible — the daemon runs as the same user, so in practice this
    /// means "no such process").
    fn open(pid: u32, access: u32) -> Option<Self> {
        // SAFETY: OpenProcess has no preconditions; a null return is checked.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: self.0 is a valid handle owned exclusively by this wrapper.
        unsafe { CloseHandle(self.0) };
    }
}

/// Whether `pid` refers to a live (not-yet-exited) process.
///
/// `OpenProcess` succeeding is NOT sufficient — the kernel process object
/// outlives exit while any handle to it is open. A zero-timeout wait
/// disambiguates: `WAIT_TIMEOUT` = still running, `WAIT_OBJECT_0` = exited.
pub(crate) fn is_pid_alive(pid: u32) -> bool {
    let Some(handle) = ProcessHandle::open(pid, PROCESS_SYNCHRONIZE) else {
        return false;
    };
    // SAFETY: handle is valid for the duration of the call.
    unsafe { WaitForSingleObject(handle.0, 0) == WAIT_TIMEOUT }
}

/// Terminate `pid` (no wait). Mirrors the Unix `kill -TERM` arm's semantics:
/// `Err` when the process exists but could not be signalled, `Err` with a
/// distinguishable message when it does not exist (matching `taskkill`'s
/// non-zero exit for an unknown PID, which callers already treat as failure).
pub(crate) fn terminate_pid(pid: u32) -> Result<(), String> {
    let Some(handle) = ProcessHandle::open(pid, PROCESS_TERMINATE) else {
        return Err(format!("no such process: pid {pid}"));
    };
    // SAFETY: handle is valid; exit code 1 marks an external termination.
    if unsafe { TerminateProcess(handle.0, 1) } == 0 {
        return Err(format!(
            "TerminateProcess failed for pid {pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Terminate `pid` and block until the process object is signalled (fully
/// exited) or `timeout` elapses.
///
/// Waiting on the handle (instead of polling a process snapshot) is exact:
/// the object signals at true process teardown, after which the executable
/// file lock is released and the binary can be overwritten by an installer.
pub(crate) fn terminate_and_wait_pid(pid: u32, timeout: Duration) -> Result<(), String> {
    let Some(handle) = ProcessHandle::open(pid, PROCESS_TERMINATE | PROCESS_SYNCHRONIZE) else {
        // Already gone — the goal state, not an error, for a kill-and-wait.
        return Ok(());
    };
    // SAFETY: handle is valid; exit code 1 marks an external termination.
    if unsafe { TerminateProcess(handle.0, 1) } == 0 {
        // The process may have exited between open and terminate; the wait
        // below settles it either way.
        tracing::debug!(
            pid,
            error = %std::io::Error::last_os_error(),
            "TerminateProcess returned an error; falling through to the wait"
        );
    }
    let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: handle is valid for the duration of the call.
    match unsafe { WaitForSingleObject(handle.0, timeout_ms) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(format!(
            "daemon pid {pid} did not exit within {}ms after TerminateProcess",
            timeout.as_millis()
        )),
        other => Err(format!(
            "wait on pid {pid} failed (WAIT_EVENT {other}): {}",
            std::io::Error::last_os_error()
        )),
    }
}
