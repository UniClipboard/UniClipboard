import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { setLayout, setZoom, platform, adjustUiScale } = vi.hoisted(() => ({
  adjustUiScale: vi.fn(),
  setLayout: vi.fn().mockResolvedValue(undefined),
  setZoom: vi.fn().mockResolvedValue(undefined),
  platform: { isLinux: true, isTauri: true },
}))
vi.mock('@/lib/platform', () => ({ detectPlatformInfo: () => platform }))
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom }) }))
vi.mock('@/lib/ipc', () => ({ commands: { setQuickPanelLayout: setLayout } }))
vi.mock('@/lib/ui-scale', () => ({
  readStoredUiScale: () => 1.25,
  adjustUiScale,
}))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ warn: vi.fn() }) }))

const storageKey = 'uniclipboard.quickPanel.windowScale'
let cleanup: (() => void) | undefined

function press(key: string, options: KeyboardEventInit = {}) {
  const event = new KeyboardEvent('keydown', { key, ctrlKey: true, cancelable: true, ...options })
  window.dispatchEvent(event)
  return event
}

beforeEach(() => {
  vi.resetModules()
  setLayout.mockClear()
  setZoom.mockClear()
  adjustUiScale.mockClear()
  platform.isLinux = true
  localStorage.clear()
})
afterEach(() => {
  cleanup?.()
  cleanup = undefined
  vi.restoreAllMocks()
})

describe('quick panel window resize', () => {
  it('reports both percentages for window, text and limit adjustments', async () => {
    const report = vi.fn()
    const { installWindowResizeShortcuts } = await import('../window-layout')
    cleanup = installWindowResizeShortcuts(report)
    press('=')
    expect(report).toHaveBeenLastCalledWith({ textPercent: 125, windowPercent: 110 })
    adjustUiScale.mockReturnValue(1.5)
    press('+', { shiftKey: true })
    expect(report).toHaveBeenLastCalledWith({ textPercent: 150, windowPercent: 110 })
    for (let i = 0; i < 10; i++) press('=')
    report.mockClear()
    press('=')
    expect(report).toHaveBeenCalledExactlyOnceWith({ textPercent: 125, windowPercent: 150 })
  })

  it('resizes with plus, equals and minus using a separate panel factor', async () => {
    const { installWindowResizeShortcuts } = await import('../window-layout')
    cleanup = installWindowResizeShortcuts()
    expect(press('=').defaultPrevented).toBe(true)
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.1)
    press('=')
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.2)
    press('-', { code: 'NumpadSubtract' })
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.1)
    expect(setZoom).not.toHaveBeenCalled()
  })

  it.each([
    ['+', 'in'],
    ['=', 'in'],
    ['-', 'out'],
    ['_', 'out'],
  ])(
    'uses Ctrl+Shift+%s for UI scale without changing the saved window size',
    async (key, direction) => {
      localStorage.setItem(storageKey, '1.2')
      const { installWindowResizeShortcuts } = await import('../window-layout')
      cleanup = installWindowResizeShortcuts()
      expect(press(key, { shiftKey: true }).defaultPrevented).toBe(true)
      expect(adjustUiScale).toHaveBeenCalledExactlyOnceWith(direction)
      expect(setLayout).not.toHaveBeenCalled()
      expect(localStorage.getItem(storageKey)).toBe('1.2')
    }
  )

  it('restores window size without changing content zoom and leaves other platforms unchanged', async () => {
    localStorage.setItem(storageKey, '1.2')
    const { setQuickPanelLayout } = await import('../window-layout')
    await setQuickPanelLayout(1.25, false)
    expect(setZoom).not.toHaveBeenCalled()
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.2)
    platform.isLinux = false
    setZoom.mockClear()
    await setQuickPanelLayout(1.25, true)
    expect(setZoom).not.toHaveBeenCalled()
    expect(setLayout).toHaveBeenLastCalledWith(1.25, true, 1)
  })

  it('restores the saved size on later layouts and after module reload', async () => {
    let layout = await import('../window-layout')
    cleanup = layout.installWindowResizeShortcuts()
    press('+')
    expect(localStorage.getItem(storageKey)).toBe('1.1')
    await layout.setQuickPanelLayout(1, true)
    expect(setLayout).toHaveBeenLastCalledWith(1, true, 1.1)
    cleanup()
    vi.resetModules()
    layout = await import('../window-layout')
    await layout.setQuickPanelLayout(1, false)
    expect(setLayout).toHaveBeenLastCalledWith(1, false, 1.1)
  })

  it('bounds repeated adjustments and still suppresses browser zoom at the limit', async () => {
    const { installWindowResizeShortcuts } = await import('../window-layout')
    cleanup = installWindowResizeShortcuts()
    for (let i = 0; i < 20; i++) press('+')
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.5)
    expect(press('+').defaultPrevented).toBe(true)
    for (let i = 0; i < 20; i++) press('-')
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 0.8)
  })

  it.each(['invalid', '', 'NaN', 'Infinity'])('ignores corrupt saved size %s', async value => {
    localStorage.setItem(storageKey, value)
    const { setQuickPanelLayout } = await import('../window-layout')
    await setQuickPanelLayout(1, false)
    expect(setLayout).toHaveBeenLastCalledWith(1, false, 1)
  })

  it('ignores unrelated shortcuts and removes its listener on cleanup', async () => {
    const { installWindowResizeShortcuts } = await import('../window-layout')
    cleanup = installWindowResizeShortcuts()
    expect(press('+', { ctrlKey: false }).defaultPrevented).toBe(false)
    press('+', { altKey: true })
    press('+', { metaKey: true })
    press('+', { isComposing: true })
    press('a')
    expect(setLayout).not.toHaveBeenCalled()
    cleanup()
    press('+')
    expect(setLayout).not.toHaveBeenCalled()
  })

  it('keeps resizing within the session if persistent storage is unavailable', async () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('unavailable')
    })
    const { installWindowResizeShortcuts, setQuickPanelLayout } = await import('../window-layout')
    cleanup = installWindowResizeShortcuts()
    press('+')
    press('+')
    await setQuickPanelLayout(1, false)
    expect(setLayout).toHaveBeenLastCalledWith(1, false, 1.2)
  })
})
