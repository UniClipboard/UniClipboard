import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  readWindowFramePreference,
  resolveWindowFrameMode,
  setStoredWindowFramePreference,
  WINDOW_FRAME_STORAGE_KEY,
} from '@/lib/window-frame'

const linuxPlatform = {
  isWindows: false,
  isMac: false,
  isLinux: true,
  isTauri: true,
}

describe('window frame preference', () => {
  const originalLocalStorageDescriptor = Object.getOwnPropertyDescriptor(window, 'localStorage')

  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    if (originalLocalStorageDescriptor) {
      Object.defineProperty(window, 'localStorage', originalLocalStorageDescriptor)
    }
  })

  it('defaults to the custom frame', () => {
    expect(readWindowFramePreference()).toBe('auto')
    expect(resolveWindowFrameMode(linuxPlatform, 'custom')).toMatchObject({
      canChooseSystemFrame: true,
      hasCustomTitleBar: true,
      hasCustomWindowControls: true,
      hasRoundedWindow: false,
      searchInTitleBar: true,
    })
  })

  it('keeps custom frame rounding as a Windows-specific policy', () => {
    expect(
      resolveWindowFrameMode({ ...linuxPlatform, isLinux: false, isWindows: true }, 'custom')
    ).toMatchObject({
      hasCustomWindowControls: true,
      hasRoundedWindow: true,
    })
  })

  it('uses native chrome and keeps search in the page when the system frame is enabled', () => {
    expect(resolveWindowFrameMode(linuxPlatform, 'system')).toMatchObject({
      canChooseSystemFrame: true,
      hasCustomTitleBar: false,
      hasCustomWindowControls: false,
      hasRoundedWindow: false,
      searchInTitleBar: false,
    })
  })

  it('persists the local window preference', () => {
    setStoredWindowFramePreference('system')

    expect(localStorage.getItem(WINDOW_FRAME_STORAGE_KEY)).toBe('system')
    expect(readWindowFramePreference()).toBe('system')
  })

  it('keeps the selected window frame for the current session when storage fails', () => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: {
        getItem: () => {
          throw new Error('Storage is unavailable')
        },
        setItem: () => {
          throw new Error('Storage is unavailable')
        },
      },
    })

    setStoredWindowFramePreference('system')

    expect(readWindowFramePreference()).toBe('system')
  })

  it('keeps the existing macOS title bar behavior', () => {
    expect(
      resolveWindowFrameMode({ ...linuxPlatform, isLinux: false, isMac: true }, 'system')
    ).toMatchObject({
      canChooseSystemFrame: false,
      hasCustomTitleBar: true,
      hasCustomWindowControls: false,
      hasRoundedWindow: false,
      searchInTitleBar: true,
    })
  })
})
