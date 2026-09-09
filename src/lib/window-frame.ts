import type { PlatformInfo } from '@/lib/platform'

export const WINDOW_FRAME_STORAGE_KEY = 'uniclipboard.useSystemWindowFrame'
const WINDOW_FRAME_CHANGED_EVENT = 'uniclipboard:window-frame-changed'
export type WindowFramePreference = 'auto' | 'custom' | 'system' | 'none'
let sessionPreference: WindowFramePreference | undefined

declare global {
  interface Window {
    __UC_WINDOW_FRAME_DEFAULT__?: 'custom' | 'none'
  }
}

type WindowFramePlatform = Pick<PlatformInfo, 'isWindows' | 'isMac' | 'isLinux' | 'isTauri'>

export interface WindowFrameMode {
  useSystemWindowFrame: boolean
  canChooseSystemFrame: boolean
  hasCustomTitleBar: boolean
  hasCustomWindowControls: boolean
  hasRoundedWindow: boolean
  searchInTitleBar: boolean
}

const getStorage = (): Storage | null => {
  if (typeof window === 'undefined') return null

  try {
    return window.localStorage
  } catch {
    return null
  }
}

export const readWindowFramePreference = (): WindowFramePreference => {
  if (sessionPreference !== undefined) return sessionPreference

  try {
    const stored = getStorage()?.getItem(WINDOW_FRAME_STORAGE_KEY)
    // Keep existing explicit choices while treating missing preferences as automatic.
    if (stored === 'true') return 'system'
    if (stored === 'false') return 'custom'
    if (stored === 'custom' || stored === 'system' || stored === 'none') return stored
    return 'auto'
  } catch {
    return 'auto'
  }
}

export const setStoredWindowFramePreference = (preference: WindowFramePreference): void => {
  const storage = getStorage()

  if (!storage) {
    sessionPreference = preference
  } else {
    try {
      storage.setItem(WINDOW_FRAME_STORAGE_KEY, preference)
      sessionPreference = undefined
    } catch {
      // A storage failure must not prevent the current window from changing frame mode.
      sessionPreference = preference
    }
  }

  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent(WINDOW_FRAME_CHANGED_EVENT))
  }
}

export const subscribeWindowFrameChanges = (listener: () => void): (() => void) => {
  if (typeof window === 'undefined') return () => {}

  const handleStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === WINDOW_FRAME_STORAGE_KEY) listener()
  }

  window.addEventListener(WINDOW_FRAME_CHANGED_EVENT, listener)
  window.addEventListener('storage', handleStorage)

  return () => {
    window.removeEventListener(WINDOW_FRAME_CHANGED_EVENT, listener)
    window.removeEventListener('storage', handleStorage)
  }
}

export const resolveWindowFrameMode = (
  platform: WindowFramePlatform,
  preference: WindowFramePreference,
  prefersNoTitleBar = typeof window !== 'undefined' && window.__UC_WINDOW_FRAME_DEFAULT__ === 'none'
): WindowFrameMode => {
  const canChooseSystemFrame = platform.isTauri && (platform.isWindows || platform.isLinux)
  const selected =
    preference === 'auto' ? (platform.isLinux && prefersNoTitleBar ? 'none' : 'custom') : preference
  const useSystemWindowFrame = canChooseSystemFrame && selected === 'system'
  const usesSelectableCustomFrame = canChooseSystemFrame && selected === 'custom'
  const hasCustomTitleBar = platform.isMac || !platform.isTauri || usesSelectableCustomFrame

  return {
    useSystemWindowFrame,
    canChooseSystemFrame,
    hasCustomTitleBar,
    hasCustomWindowControls: usesSelectableCustomFrame,
    hasRoundedWindow: platform.isWindows && usesSelectableCustomFrame,
    searchInTitleBar: platform.isMac || usesSelectableCustomFrame,
  }
}

export const applyWindowFrameDocumentState = (hasRoundedWindow: boolean): void => {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.ucCustomWindowFrame = String(hasRoundedWindow)
}
