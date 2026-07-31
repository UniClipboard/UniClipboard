if (import.meta.env.VITE_E2E === '1') {
  await import('@wdio/tauri-plugin')
}

interface WdioE2eEvent {
  at: number
  name: string
  detail?: unknown
}

declare global {
  interface Window {
    __WDIO_E2E_EVENTS__?: WdioE2eEvent[]
  }
}

export function recordWdioE2eEvent(name: string, detail?: unknown) {
  if (import.meta.env.VITE_E2E !== '1' || typeof window === 'undefined') return
  const events = (window.__WDIO_E2E_EVENTS__ ??= [])
  events.push({ at: Date.now(), name, detail })
}
