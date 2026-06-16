; NSIS installer hooks for UniClipboard (Tauri `bundle.windows.nsis.installerHooks`).
;
; Why this exists:
;   Since ADR-008 the clipboard engine ships as a standalone `uniclipd.exe`
;   sidecar (bundled via `externalBin`). The GUI (`UniClipboard.exe`) spawns it
;   as a detached process. A manual `setup.exe` run over a live install must
;   stop BOTH before file extraction, otherwise NSIS hits a file lock and raises
;   the "file in use — Abort/Retry/Ignore" dialog (aborting then freezes the
;   installer). The in-app updater handles this via stop_daemon_before_update();
;   a manual installer run bypasses that path entirely.
;
; Two things must happen, in order:
;   1. Kill the GUI FIRST. It is the daemon's parent/supervisor; killing only
;      the daemon leaves the GUI able to respawn it, and the GUI can also hold
;      OS handles that keep `uniclipd.exe` locked even after the daemon process
;      is gone from Task Manager.
;   2. Kill the daemon, then WAIT until its binary is actually writable. A
;      force-killed (taskkill /F → TerminateProcess) process releases its
;      image-file lock asynchronously, and AV (e.g. Defender) may briefly scan
;      the file — so a fixed Sleep races. We poll the target until it opens for
;      write (or give up after ~10s and let NSIS surface a clear error).
;
; Graceful, PID-identity-aware shutdown remains the in-app updater's job; the
; installer can only match by image name and force-terminate.

; UNIQ makes the internal labels unique per insertion — a label-bearing macro
; inserted more than once in the same script (PREINSTALL + PREUNINSTALL) would
; otherwise raise "label already defined".
!macro UC_STOP_AND_WAIT UNIQ
  DetailPrint "Stopping UniClipboard (GUI + daemon) before install..."
  ; 1) GUI first (taskkill image match is case-insensitive, so this also
  ;    covers an exe named uniclipboard.exe).
  nsExec::Exec 'taskkill /F /T /IM UniClipboard.exe'
  Pop $0
  ; 2) Daemon.
  nsExec::Exec 'taskkill /F /T /IM uniclipd.exe'
  Pop $0

  ; 3) Wait until the existing daemon binary is unlocked. Skip on a fresh
  ;    install where the file is absent.
  Push $0
  Push $R0
  IfFileExists "$INSTDIR\uniclipd.exe" 0 uc_unlock_done_${UNIQ}
  StrCpy $R0 0
  uc_wait_unlock_${UNIQ}:
    ClearErrors
    FileOpen $0 "$INSTDIR\uniclipd.exe" a
    IfErrors uc_locked_${UNIQ}
    FileClose $0
    Goto uc_unlock_done_${UNIQ}
  uc_locked_${UNIQ}:
    Sleep 500
    IntOp $R0 $R0 + 1
    ; 20 * 500ms = ~10s ceiling, then fall through and let NSIS try the write.
    IntCmp $R0 20 uc_unlock_done_${UNIQ} uc_wait_unlock_${UNIQ} uc_unlock_done_${UNIQ}
  uc_unlock_done_${UNIQ}:
  Pop $R0
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UC_STOP_AND_WAIT "install"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UC_STOP_AND_WAIT "uninstall"
!macroend
