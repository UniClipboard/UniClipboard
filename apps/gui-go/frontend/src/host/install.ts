// Marks the WebView as the desktop host before the frontend reads platform flags.
;(window as unknown as { __UC_DESKTOP_HOST__: boolean }).__UC_DESKTOP_HOST__ = true

// The Go host shows the main window itself and has a single window generation, so the
// readiness handshake the shared app performs (`mark_main_window_ready`) always carries "1".
;(window as unknown as { __UC_MAIN_WINDOW_GENERATION__: string }).__UC_MAIN_WINDOW_GENERATION__ =
  '1'
