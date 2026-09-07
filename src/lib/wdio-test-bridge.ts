if (import.meta.env.VITE_E2E === '1') {
  await import('@wdio/tauri-plugin')
  const NativeSocket = window.WebSocket
  const sockets = new Set<WebSocket>()
  let blockedEndpoint: string | null = null
  const isBlocked = (socket: WebSocket) =>
    blockedEndpoint !== null && socket.url.split('?')[0] === blockedEndpoint
  window.WebSocket = class extends NativeSocket {
    constructor(url: string | URL, protocols?: string | string[]) {
      super(url, protocols)
      sockets.add(this)
      this.addEventListener('close', () => sockets.delete(this), { once: true })
      this.addEventListener('open', () => {
        if (isBlocked(this)) this.close()
      })
    }
  }
  window.__WDIO_E2E_NETWORK__ = {
    disconnect(endpoint) {
      const parsed = new URL(endpoint)
      if (!['127.0.0.1', 'localhost', '[::1]'].includes(parsed.hostname))
        throw new Error('Only local test endpoints may be disconnected')
      blockedEndpoint = endpoint
      for (const socket of sockets) if (isBlocked(socket)) socket.close()
    },
    restore() {
      blockedEndpoint = null
    },
  }
}

interface WdioE2eEvent {
  at: number
  name: string
  detail?: unknown
}

declare global {
  interface Window {
    __WDIO_E2E_EVENTS__?: WdioE2eEvent[]
    __WDIO_E2E_NETWORK__?: { disconnect: (endpoint: string) => void; restore: () => void }
  }
}

export function recordWdioE2eEvent(name: string, detail?: unknown) {
  if (import.meta.env.VITE_E2E !== '1' || typeof window === 'undefined') return
  const events = (window.__WDIO_E2E_EVENTS__ ??= [])
  events.push({ at: Date.now(), name, detail })
}
