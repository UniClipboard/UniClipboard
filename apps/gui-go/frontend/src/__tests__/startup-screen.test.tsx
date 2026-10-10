import { act, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { THEME_MODE_STORAGE_KEY } from '@/lib/theme-mode-cache'

vi.mock('@/lib/platform', () => ({
  detectPlatformInfo: () => ({
    isLinux: false,
    isWindows: false,
    isMac: true,
    isDesktopHost: true,
  }),
}))

beforeEach(() => {
  vi.resetModules()
  document.body.innerHTML = '<div id="root"><div class="uc-splash"></div></div>'
  document.documentElement.className = ''
  window.localStorage.clear()
})

afterEach(() => {
  document.body.replaceChildren()
})

it('replaces the static splash with the startup screen using the cached theme mode', async () => {
  window.localStorage.setItem(THEME_MODE_STORAGE_KEY, 'dark')
  const { showStartupScreen } = await import('@/startup-screen')

  await act(async () => showStartupScreen())

  expect(document.documentElement).toHaveClass('dark')
  expect(document.querySelector('.uc-splash')).toBeNull()
  expect(screen.getByRole('heading', { level: 1 })).toBeInTheDocument()
})

it('lets the full app take over the same root without a second createRoot', async () => {
  const { showStartupScreen } = await import('@/startup-screen')
  const { getAppRoot } = await import('@/app-root')
  await act(async () => showStartupScreen())

  await act(async () => getAppRoot().render(<main>App content</main>))

  expect(screen.getByText('App content')).toBeInTheDocument()
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull()
})
