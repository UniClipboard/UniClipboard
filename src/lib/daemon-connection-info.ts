import { commands } from '@/lib/ipc'
import type { DaemonConnectionPayload as GeneratedDaemonConnectionPayload } from '@/lib/ipc'

const POLL_INTERVAL_MS = 500

export type DaemonConnectionPayload = GeneratedDaemonConnectionPayload

let connectionInfoPromise: Promise<DaemonConnectionPayload> | null = null

export function waitForDaemonConnectionInfo(): Promise<DaemonConnectionPayload> {
  if (connectionInfoPromise) {
    return connectionInfoPromise
  }

  connectionInfoPromise = pollForDaemonConnectionInfo().catch(error => {
    connectionInfoPromise = null
    throw error
  })

  return connectionInfoPromise
}

export function resetDaemonConnectionInfoPollingForTests(): void {
  connectionInfoPromise = null
}

async function pollForDaemonConnectionInfo(): Promise<DaemonConnectionPayload> {
  while (true) {
    const payload = await commands.getDaemonConnectionInfo()
    if (payload) {
      validatePayload(payload)
      return payload
    }

    await sleep(POLL_INTERVAL_MS)
  }
}

function validatePayload(payload: unknown): asserts payload is DaemonConnectionPayload {
  if (
    typeof payload !== 'object' ||
    payload === null ||
    !('baseUrl' in payload) ||
    !('wsUrl' in payload) ||
    typeof (payload as DaemonConnectionPayload).baseUrl !== 'string' ||
    typeof (payload as DaemonConnectionPayload).wsUrl !== 'string' ||
    !(payload as DaemonConnectionPayload).baseUrl ||
    !(payload as DaemonConnectionPayload).wsUrl
  ) {
    throw new Error('Malformed daemon connection payload: missing required fields')
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}
