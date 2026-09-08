import { emit, listen } from '@tauri-apps/api/event'
import { createLogger } from '@/lib/logger'

export const DEVICE_SYNC_CHANGED_EVENT = 'devices://sync-changed'
const log = createLogger('device-sync-events')

export function subscribeDeviceSyncChanged(deviceId: string, onChange: () => void): () => void {
  let disposed = false
  const subscription = listen<string>(DEVICE_SYNC_CHANGED_EVENT, event => {
    if (!disposed && event.payload === deviceId) onChange()
  }).catch(err => {
    log.error({ err }, 'Failed to subscribe to device sync changes')
    return () => {}
  })
  return () => {
    disposed = true
    void subscription.then(unlisten => unlisten())
  }
}

export async function emitDeviceSyncChanged(deviceId: string): Promise<void> {
  try {
    await emit(DEVICE_SYNC_CHANGED_EVENT, deviceId)
  } catch (err) {
    log.error({ err }, 'Failed to broadcast device sync change')
  }
}
