import { getCurrentWindow } from '@tauri-apps/api/window'
import { createLogger } from '@/lib/logger'
import { detectPlatformInfo } from '@/lib/platform'
import {
  applyWindowFrameDocumentState,
  readUseSystemWindowFrame,
  resolveWindowFrameMode,
} from '@/lib/window-frame'

const log = createLogger('window-frame')

export const initializeWindowFrame = (): void => {
  const platform = detectPlatformInfo()
  const useSystemWindowFrame = readUseSystemWindowFrame()
  const mode = resolveWindowFrameMode(platform, useSystemWindowFrame)

  applyWindowFrameDocumentState(mode.hasRoundedWindow)

  if (!mode.canChooseSystemFrame) return

  getCurrentWindow()
    .setDecorations(useSystemWindowFrame)
    .catch(error => log.error({ err: error }, 'Failed to initialize window frame'))
}
