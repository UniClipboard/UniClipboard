import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCallback, useMemo, useSyncExternalStore } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import {
  applyWindowFrameDocumentState,
  readWindowFramePreference,
  resolveWindowFrameMode,
  setStoredWindowFramePreference,
  subscribeWindowFrameChanges,
  type WindowFramePreference,
} from '@/lib/window-frame'

export function useWindowFrame() {
  const platform = usePlatform()
  const windowFramePreference = useSyncExternalStore(
    subscribeWindowFrameChanges,
    readWindowFramePreference,
    () => 'auto' as const
  )
  const mode = useMemo(
    () => resolveWindowFrameMode(platform, windowFramePreference),
    [platform, windowFramePreference]
  )

  const setWindowFramePreference = useCallback(
    async (preference: WindowFramePreference) => {
      if (!mode.canChooseSystemFrame) return

      const nextMode = resolveWindowFrameMode(platform, preference)
      await getCurrentWindow().setDecorations(nextMode.useSystemWindowFrame)
      setStoredWindowFramePreference(preference)
      applyWindowFrameDocumentState(nextMode.hasRoundedWindow)
    },
    [mode.canChooseSystemFrame, platform]
  )

  return {
    ...mode,
    windowFramePreference,
    setWindowFramePreference,
  }
}
