import { getCurrentWindow } from '@tauri-apps/api/window'
import { onAction } from '@tauri-apps/plugin-notification'
import { useEffect } from 'react'
import type { DeviceTrustSnapshot } from '@/api/daemon/device-trust'
import {
  DEVICE_TRUST_NOTIFICATION_ID,
  notifyDeviceTrustSnapshot,
} from '@/lib/device-trust-notifications'
import { detectPlatformInfo } from '@/lib/platform'

async function focusMainWindow(): Promise<void> {
  try {
    const window = getCurrentWindow()
    await window.show()
    await window.unminimize()
    await window.setFocus()
  } catch {
    // The decision remains available when the main window is shown manually.
  }
}

export function useDeviceTrustDesktopEffects(snapshot: DeviceTrustSnapshot | null): void {
  const isTauri = detectPlatformInfo().isTauri

  useEffect(() => {
    if (!snapshot) return
    void notifyDeviceTrustSnapshot(snapshot)
    if (snapshot.currentChange && isTauri) void focusMainWindow()
  }, [isTauri, snapshot])

  useEffect(() => {
    if (!isTauri) return
    let disposed = false
    let unregister: (() => void) | null = null
    void onAction(notification => {
      if (notification.id === DEVICE_TRUST_NOTIFICATION_ID) void focusMainWindow()
    })
      .then(listener => {
        if (disposed) listener.unregister()
        else unregister = () => listener.unregister()
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unregister?.()
    }
  }, [isTauri])
}
