import { useEffect } from 'react'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { detectPlatformInfo } from '@/lib/platform'

declare global {
  interface Window {
    __UC_MAIN_WINDOW_GENERATION__?: string
  }
}

const log = createLogger('main-window-ready')

export function MainWindowReady() {
  useEffect(() => {
    if (!detectPlatformInfo().isTauri) return
    const generation = window.__UC_MAIN_WINDOW_GENERATION__
    if (!generation) {
      log.error('Missing main window generation at startup')
      return
    }

    // Effects run after the DOM commit, including an error boundary's fallback.
    // Do not wait for requestAnimationFrame: hidden webviews may suspend it.
    commands.markMainWindowReady(generation).catch(error => {
      log.error({ err: error }, 'Failed to acknowledge main window readiness')
    })
  }, [])

  return null
}
