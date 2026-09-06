import { getCurrentWindow } from '@tauri-apps/api/window'
import { useCallback, useMemo, useSyncExternalStore } from 'react'
import { usePlatform } from '@/hooks/usePlatform'
import {
  applyWindowFrameDocumentState,
  readUseSystemWindowFrame,
  resolveWindowFrameMode,
  setStoredUseSystemWindowFrame,
  subscribeWindowFrameChanges,
} from '@/lib/window-frame'

export function useWindowFrame() {
  const platform = usePlatform()
  const useSystemWindowFrame = useSyncExternalStore(
    subscribeWindowFrameChanges,
    readUseSystemWindowFrame,
    () => false
  )
  const mode = useMemo(
    () => resolveWindowFrameMode(platform, useSystemWindowFrame),
    [platform, useSystemWindowFrame]
  )

  const setUseSystemWindowFrame = useCallback(
    async (enabled: boolean) => {
      if (!mode.canChooseSystemFrame) return

      await getCurrentWindow().setDecorations(enabled)
      setStoredUseSystemWindowFrame(enabled)
      applyWindowFrameDocumentState(resolveWindowFrameMode(platform, enabled).hasRoundedWindow)
    },
    [mode.canChooseSystemFrame, platform]
  )

  return {
    ...mode,
    useSystemWindowFrame,
    setUseSystemWindowFrame,
  }
}
