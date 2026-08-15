import { isPermissionGranted, sendNotification } from '@tauri-apps/plugin-notification'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import i18n from '@/i18n'

const notified = new Set<string>()
const inFlight = new Set<string>()
export const DEVICE_TRUST_NOTIFICATION_ID = 21021

export async function notifyDeviceTrustSnapshot(snapshot: DeviceTrustSnapshot): Promise<void> {
  const keys: string[] = []
  if (snapshot.currentChange) keys.push(`change:${snapshot.currentChange.changeId}`)
  for (const device of snapshot.devices) {
    if (device.compatibility === 'upgrade_required') keys.push(`upgrade:${device.deviceId}`)
    if (device.groupRelationship === 'diverged') keys.push(`diverged:${device.deviceId}`)
    if (device.groupRelationship === 'unverifiable') keys.push(`unverifiable:${device.deviceId}`)
  }
  const fresh = keys.filter(key => !notified.has(key) && !inFlight.has(key))
  if (fresh.length === 0) return
  fresh.forEach(key => inFlight.add(key))
  try {
    if (!(await isPermissionGranted())) return
    sendNotification({
      id: DEVICE_TRUST_NOTIFICATION_ID,
      title: i18n.t('deviceTrust.notification.title'),
      body: snapshot.currentChange
        ? i18n.t('deviceTrust.notification.decision')
        : i18n.t('deviceTrust.notification.status'),
    })
    fresh.forEach(key => notified.add(key))
  } catch {
    // Notifications are best-effort; the in-app state remains authoritative.
  } finally {
    fresh.forEach(key => inFlight.delete(key))
  }
}
