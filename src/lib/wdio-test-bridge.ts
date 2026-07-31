if (import.meta.env.VITE_E2E === '1') {
  await import('@wdio/tauri-plugin')
}
