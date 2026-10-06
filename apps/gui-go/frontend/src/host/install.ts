// Marks the WebView as a desktop shell before the shared frontend reads platform flags.
;(window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {}
