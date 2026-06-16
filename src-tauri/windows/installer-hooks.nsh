; NSIS installer hooks for UniClipboard (Tauri `bundle.windows.nsis.installerHooks`).
;
; Why this exists:
;   Since ADR-008 the clipboard engine ships as a standalone `uniclipd.exe`
;   sidecar (bundled via `externalBin`) that runs detached and usually outlives
;   the GUI (autostart). Tauri's default NSIS template only stops the main
;   binary (`UniClipboard.exe`); it has no knowledge of the daemon. So a manual
;   `setup.exe` run over a live install hits a file lock on `uniclipd.exe`,
;   raising the "file in use — Abort/Retry/Ignore" dialog. Aborting just cancels
;   the install and leaves the daemon running.
;
;   The in-app updater handles this via `stop_daemon_before_update()` (PID-exact,
;   graceful drain). A manual installer run bypasses that path entirely, so we
;   stop the daemon here as well.
;
; Why force-kill by image name:
;   The installer can't resolve the daemon PID, and `uniclipd.exe` is windowless
;   so a graceful `taskkill` (WM_CLOSE) cannot reach it. Replacing the binary
;   requires the lock released before file extraction, so we force-terminate by
;   image name. `/T` also clears any child processes. Graceful, PID-identity-aware
;   shutdown remains the in-app updater's job.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Stopping UniClipboard background service (uniclipd.exe)..."
  nsExec::Exec 'taskkill /F /T /IM uniclipd.exe'
  Pop $0 ; discard exit code: a non-zero "not running" result is fine
  Sleep 500 ; give Windows a moment to release the file lock
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Stopping UniClipboard background service (uniclipd.exe)..."
  nsExec::Exec 'taskkill /F /T /IM uniclipd.exe'
  Pop $0
  Sleep 500
!macroend
