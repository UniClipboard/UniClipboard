import type { MobileDeviceView } from '@/api/tauri-command/mobile_sync'

const MOBILE_ACTIVE_WINDOW_MS = 10 * 60 * 1000

/** A phone counts as active when it synced within the last ten minutes. */
export function isMobileDeviceActive(device: MobileDeviceView, now: number): boolean {
  return device.lastSeenAtMs != null && now - device.lastSeenAtMs <= MOBILE_ACTIVE_WINDOW_MS
}
