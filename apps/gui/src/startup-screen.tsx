import React from 'react'
import { getAppRoot } from '@/app-root'
import { AppStateFrame } from '@/components/app/AppStateFrame'
import { StartupProgressScreen } from '@/components/app/StartupProgressScreen'
import '@/i18n'
import WindowShell from '@/layouts/WindowShell'
import { detectPlatformInfo } from '@/lib/platform'
import { pendingStartupSnapshot } from '@/lib/startup-progress'
import { readCachedThemeMode } from '@/lib/theme-mode-cache'
import { readWindowFramePreference, resolveWindowFrameMode } from '@/lib/window-frame'
import '@/App.css'

/**
 * Renders the startup screen before the rest of the app has loaded. Keep this module's import
 * graph small (no IPC, diagnostics, store or router): it exists so the first React frame does not wait for the whole application.
 * It mirrors the layout of the startup view in `AppContentView`.
 */
export function showStartupScreen(): void {
  // The full theme needs IPC; the cached light/dark mode is enough for this first screen.
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(readCachedThemeMode())
  const { hasCustomTitleBar } = resolveWindowFrameMode(
    detectPlatformInfo(),
    readWindowFramePreference()
  )
  const titleBar = hasCustomTitleBar ? <div className="h-10 shrink-0" /> : null
  getAppRoot().render(
    <React.StrictMode>
      <WindowShell titleBar={null}>
        <div className="relative h-full w-full overflow-hidden">
          <div className="absolute inset-0 flex min-h-0">
            <AppStateFrame titleBar={titleBar}>
              <StartupProgressScreen
                snapshot={pendingStartupSnapshot}
                onRetry={() => {}}
                onExport={async () => {
                  const { exportStartupLogs } = await import('@/api/startup-support')
                  return (await exportStartupLogs()) !== null
                }}
              />
            </AppStateFrame>
          </div>
        </div>
      </WindowShell>
    </React.StrictMode>
  )
}
