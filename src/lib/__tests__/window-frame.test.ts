import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  readUseSystemWindowFrame,
  resolveWindowFrameMode,
  setStoredUseSystemWindowFrame,
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
    expect(readUseSystemWindowFrame()).toBe(false)
    expect(resolveWindowFrameMode(linuxPlatform, false)).toEqual({
      canChooseSystemFrame: true,
      hasCustomTitleBar: true,
      hasCustomWindowControls: true,
      hasRoundedWindow: true,
      searchInTitleBar: true,
    })
  })

  it('uses native chrome and keeps search in the page when the system frame is enabled', () => {
    expect(resolveWindowFrameMode(linuxPlatform, true)).toEqual({
      canChooseSystemFrame: true,
      hasCustomTitleBar: false,
      hasCustomWindowControls: false,
      hasRoundedWindow: false,
      searchInTitleBar: false,
    })
  })

  it('persists the local window preference', () => {
    setStoredUseSystemWindowFrame(true)

    expect(localStorage.getItem(WINDOW_FRAME_STORAGE_KEY)).toBe('true')
    expect(readUseSystemWindowFrame()).toBe(true)
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

    setStoredUseSystemWindowFrame(true)

    expect(readUseSystemWindowFrame()).toBe(true)
  })

  it('keeps the existing macOS title bar behavior', () => {
    expect(resolveWindowFrameMode({ ...linuxPlatform, isLinux: false, isMac: true }, true)).toEqual(
      {
        canChooseSystemFrame: false,
        hasCustomTitleBar: true,
        hasCustomWindowControls: false,
        hasRoundedWindow: false,
        searchInTitleBar: true,
      }
    )
  })
})
