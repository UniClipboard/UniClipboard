import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { setLayout, platform, adjustUiScale } = vi.hoisted(() => ({
  setLayout: vi.fn().mockResolvedValue(undefined),
  adjustUiScale: vi.fn().mockReturnValue(1.5),
  platform: { isLinux: true, isTauri: true },
}))
vi.mock('@/lib/platform', () => ({ detectPlatformInfo: () => platform }))
vi.mock('@/lib/ipc', () => ({ commands: { setQuickPanelLayout: setLayout } }))
vi.mock('@/lib/ui-scale', () => ({ readStoredUiScale: () => 1.25, adjustUiScale }))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ warn: vi.fn() }) }))
const storageKey = 'uniclipboard.quickPanel.windowScale'

beforeEach(() => {
  vi.resetModules()
  setLayout.mockClear()
  adjustUiScale.mockClear()
  platform.isLinux = true
  localStorage.clear()
})
afterEach(() => vi.restoreAllMocks())

describe('quick panel scale actions', () => {
  it('reports both values while keeping window and text preferences independent', async () => {
    const { adjustQuickPanelScale } = await import('../window-layout')
    expect(adjustQuickPanelScale('windowIncrease')).toEqual({
      textPercent: 125,
      windowPercent: 110,
    })
    expect(setLayout).toHaveBeenLastCalledWith(1.25, false, 1.1)
    setLayout.mockClear()
    expect(adjustQuickPanelScale('textIncrease')).toEqual({ textPercent: 150, windowPercent: 110 })
    expect(adjustUiScale).toHaveBeenCalledWith('in')
    expect(setLayout).not.toHaveBeenCalled()
    expect(localStorage.getItem(storageKey)).toBe('1.1')
  })
  it('restores size after module reload and ignores the factor on other platforms', async () => {
    let layout = await import('../window-layout')
    layout.adjustQuickPanelScale('windowIncrease')
    vi.resetModules()
    layout = await import('../window-layout')
    await layout.setQuickPanelLayout(1, false)
    expect(setLayout).toHaveBeenLastCalledWith(1, false, 1.1)
    platform.isLinux = false
    await layout.setQuickPanelLayout(1, true)
    expect(setLayout).toHaveBeenLastCalledWith(1, true, 1)
  })
  it('bounds repeated adjustments and reports the limit', async () => {
    const { adjustQuickPanelScale } = await import('../window-layout')
    for (let i = 0; i < 20; i++) adjustQuickPanelScale('windowIncrease')
    expect(adjustQuickPanelScale('windowIncrease').windowPercent).toBe(150)
    for (let i = 0; i < 20; i++) adjustQuickPanelScale('windowDecrease')
    expect(adjustQuickPanelScale('windowDecrease').windowPercent).toBe(80)
  })
  it.each(['invalid', '', 'NaN', 'Infinity'])('ignores corrupt saved size %s', async value => {
    localStorage.setItem(storageKey, value)
    const { setQuickPanelLayout } = await import('../window-layout')
    await setQuickPanelLayout(1, false)
    expect(setLayout).toHaveBeenLastCalledWith(1, false, 1)
  })
  it('keeps resizing if storage is unavailable', async () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('unavailable')
    })
    const { adjustQuickPanelScale } = await import('../window-layout')
    adjustQuickPanelScale('windowIncrease')
    expect(adjustQuickPanelScale('windowIncrease').windowPercent).toBe(120)
  })
})
