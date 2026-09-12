import { useEffect, useState } from 'react'
import { subscribeDesktopTheme } from '@/lib/desktop-theme'
import type { DesktopThemeSnapshot } from '@/lib/desktop-theme'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('omarchy-theme')

export function useOmarchyTheme() {
  const [snapshot, setSnapshot] = useState<DesktopThemeSnapshot>()
  const [saving, setSaving] = useState(false)
  const [failed, setFailed] = useState(false)
  useEffect(
    () =>
      subscribeDesktopTheme((_theme, _radius, next) => {
        if (next)
          setSnapshot(previous =>
            !previous || next.revision >= previous.revision ? next : previous
          )
      }),
    []
  )
  const setEnabled = async (enabled: boolean) => {
    setSaving(true)
    setFailed(false)
    try {
      const next = await commands.setFollowOmarchyTheme(enabled)
      setSnapshot(previous => (!previous || next.revision >= previous.revision ? next : previous))
    } catch (err) {
      log.error({ err }, 'Failed to save Omarchy theme preference')
      setFailed(true)
    } finally {
      setSaving(false)
    }
  }
  return {
    enabled: snapshot?.followOmarchyTheme ?? false,
    available: snapshot?.omarchyAvailable ?? false,
    saving,
    failed,
    setEnabled,
  }
}
