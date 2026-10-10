import { useEffect, useState } from 'react'
import { listMobileDevices, type MobileDeviceView } from '@/api/tauri-command/mobile_sync'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-mobile-device-list')

/**
 * Read-only roster of registered mobile-sync devices, fetched once on mount.
 *
 * For places that only need the device list for display/selection (e.g. the
 * History source filter). This is intentionally minimal — it is not a
 * substitute for the full management hook on the devices page.
 */
export function useMobileDeviceList(): MobileDeviceView[] {
  const [devices, setDevices] = useState<MobileDeviceView[]>([])

  useEffect(() => {
    let cancelled = false
    listMobileDevices()
      .then(list => {
        if (!cancelled) setDevices(list)
      })
      .catch(err => {
        log.error({ err }, 'failed to list mobile devices')
      })
    return () => {
      cancelled = true
    }
  }, [])

  return devices
}
