import { afterEach, expect, it } from 'vitest'
import {
  readWindowFramePreference,
  setStoredWindowFramePreference,
  resolveWindowFrameMode,
  WINDOW_FRAME_STORAGE_KEY,
} from '@/lib/window-frame'

const linux = { isLinux: true, isMac: false, isWindows: false, isTauri: true }
afterEach(() => localStorage.clear())

it('defaults to automatic before setup', () => {
  expect(readWindowFramePreference()).toBe('auto')
})

it.each(['auto', 'custom', 'system', 'none'] as const)('persists the %s choice', preference => {
  setStoredWindowFramePreference(preference)
  expect(readWindowFramePreference()).toBe(preference)
  expect(localStorage.getItem(WINDOW_FRAME_STORAGE_KEY)).toBe(preference)
})

it('respects legacy explicit choices', () => {
  localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, 'false')
  expect(readWindowFramePreference()).toBe('custom')
  localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, 'true')
  expect(readWindowFramePreference()).toBe('system')
})

it('hides both frames on a tiling desktop before setup', () => {
  expect(resolveWindowFrameMode(linux, 'auto', true)).toMatchObject({
    hasCustomTitleBar: false,
    hasCustomWindowControls: false,
    searchInTitleBar: false,
    useSystemWindowFrame: false,
  })
})

it.each(['custom', 'system', 'none'] as const)(
  'honors explicit %s on a tiling desktop',
  preference => {
    expect(resolveWindowFrameMode(linux, preference, true)).toMatchObject({
      hasCustomTitleBar: preference === 'custom',
      useSystemWindowFrame: preference === 'system',
    })
  }
)

it('keeps controls on unknown desktops', () => {
  expect(resolveWindowFrameMode(linux, 'auto', false).hasCustomWindowControls).toBe(true)
})
