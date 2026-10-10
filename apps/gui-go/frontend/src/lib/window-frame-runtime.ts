import { getCurrentWindow } from '@tauri-apps/api/window'
import { createLogger } from '@/lib/logger'
import { detectPlatformInfo } from '@/lib/platform'
import { readWindowFramePreference, resolveWindowFrameMode } from '@/lib/window-frame'

const log = createLogger('window-frame')

export const initializeWindowFrame = async (): Promise<void> => {
  const platform = detectPlatformInfo()
  const mode = resolveWindowFrameMode(platform, readWindowFramePreference())

  if (!mode.canChooseSystemFrame) return

  await getCurrentWindow()
    .setDecorations(mode.useSystemWindowFrame)
    .catch(error => log.error({ err: error }, 'Failed to initialize window frame'))
}
