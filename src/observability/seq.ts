const LEVEL_MAP = {
  log: 'Information',
  warn: 'Warning',
  error: 'Error',
  debug: 'Debug',
} as const

let buffer: string[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
let serverUrl = ''
const originalWarn = console.warn.bind(console)

function flush() {
  if (buffer.length === 0) return
  const body = buffer.join('\n')
  buffer = []
  flushTimer = null
  fetch(`${serverUrl}/api/events/raw?clef`, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain' },
    body,
    keepalive: true,
  }).catch(e => originalWarn('[Seq] flush failed:', e))
}

function enqueue(event: string) {
  buffer.push(event)
  if (!flushTimer) {
    flushTimer = setTimeout(flush, 1000)
  }
}

export function initSeq() {
  if (!import.meta.env.DEV || !import.meta.env.VITE_SEQ_URL) {
    return
  }

  // Lazy import to avoid circular dependency (trace.ts may use console)
  let getTraceId: (() => string | undefined) | null = null
  import('@/observability/trace').then(m => {
    getTraceId = () => m.traceManager.getCurrentTrace()?.traceId
  })

  serverUrl = import.meta.env.VITE_SEQ_URL

  for (const [method, level] of Object.entries(LEVEL_MAP)) {
    const key = method as keyof typeof LEVEL_MAP
    const original = console[key]
    console[key] = (...args: unknown[]) => {
      original.apply(console, args)
      const clef: Record<string, string> = {
        '@t': new Date().toISOString(),
        '@l': level,
        '@mt': args.map(String).join(' '),
        Source: 'frontend',
      }
      const traceId = getTraceId?.()
      if (traceId) {
        clef.trace_id = traceId
      }
      enqueue(JSON.stringify(clef))
    }
  }

  window.addEventListener('beforeunload', flush)
}
