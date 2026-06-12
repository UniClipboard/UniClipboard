//! Suppress console-window flashes when spawning Windows console tools.
//!
//! The GUI shell is a `windows_subsystem = "windows"` process with no console
//! of its own. When such a process spawns a console-subsystem tool, Windows
//! allocates a brand-new console window for the child — a black box that
//! flashes over the user's screen (field-reported as "the app keeps popping
//! up terminal windows", at its worst ~100 flashes per update attempt from
//! the old 100ms `tasklist` poll).
//!
//! Process liveness/termination has since moved to Win32 calls
//! ([`crate::win_process`] — no child process at all); [`configure_no_window`]
//! covers the shell-outs that remain (currently the `netstat` port-lookup
//! fallback). Apply it to **every** `Command` in this crate that runs a
//! Windows console tool.
//!
//! Not used by [`crate::spawn`]: the daemon spawn needs `DETACHED_PROCESS`
//! (child must survive the parent), which already implies no console.

pub(crate) fn configure_no_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;

    // CreateProcess flag: the child gets no console window at all.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}
