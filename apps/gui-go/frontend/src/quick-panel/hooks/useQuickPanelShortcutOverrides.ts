import { useEffect, useState } from 'react'
import { subscribeSettings } from '@/lib/settings-sync'
import type { ShortcutKeyOverrides } from '@/shortcuts/conflicts'

/** The quick panel's shortcut overrides, from the window's authoritative settings copy. */
export function useQuickPanelShortcutOverrides() {
  const [overrides, setOverrides] = useState<ShortcutKeyOverrides>({})
  useEffect(
    () =>
      subscribeSettings(({ settings }) => {
        if (settings) setOverrides(settings.keyboardShortcuts ?? {})
      }),
    []
  )
  return overrides
}
