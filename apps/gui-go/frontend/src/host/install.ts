// Marks the WebView as a desktop shell before the shared frontend reads platform flags.
;(window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {}

// The Go host shows the main window itself and has a single window generation, so the
// readiness handshake the shared app performs (`mark_main_window_ready`) always carries "1".
;(window as unknown as { __UC_MAIN_WINDOW_GENERATION__: string }).__UC_MAIN_WINDOW_GENERATION__ =
  '1'
