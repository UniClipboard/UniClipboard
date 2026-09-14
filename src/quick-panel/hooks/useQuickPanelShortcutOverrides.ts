import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { getSettings } from '@/api/daemon'
import { createLogger } from '@/lib/logger'
import { parseSettingsChangedPayload, SETTINGS_CHANGED_EVENT } from '@/lib/settings-events'
import type { ShortcutKeyOverrides } from '@/shortcuts/conflicts'
import type { SettingChangedEvent } from '@/types/events'

const log = createLogger('quick-panel-shortcuts')

export function useQuickPanelShortcutOverrides() {
  const [overrides, setOverrides] = useState<ShortcutKeyOverrides>({})
  useEffect(() => {
    let cancelled = false
    let revision = 0
    const subscription = listen<SettingChangedEvent>(SETTINGS_CHANGED_EVENT, event => {
      const settings = parseSettingsChangedPayload(event.payload)
      if (cancelled || !settings) return
      revision += 1
      setOverrides(settings.keyboardShortcuts ?? {})
    })
    void subscription
      .then(async () => {
        const startedAt = revision
        const settings = await getSettings()
        if (!cancelled && startedAt === revision) setOverrides(settings.keyboardShortcuts ?? {})
      })
      .catch(err => log.warn({ err }, 'failed to load quick panel shortcuts'))
    return () => {
      cancelled = true
      void subscription.then(dispose => dispose()).catch(() => {})
    }
  }, [])
  return overrides
}
