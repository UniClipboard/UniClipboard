import type { PlatformInfo } from '@/lib/platform'

export const WINDOW_FRAME_STORAGE_KEY = 'uniclipboard.useSystemWindowFrame'
const WINDOW_FRAME_CHANGED_EVENT = 'uniclipboard:window-frame-changed'

type WindowFramePlatform = Pick<PlatformInfo, 'isWindows' | 'isMac' | 'isLinux' | 'isTauri'>

export interface WindowFrameMode {
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

export const readUseSystemWindowFrame = (): boolean => {
  try {
    return getStorage()?.getItem(WINDOW_FRAME_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

export const setStoredUseSystemWindowFrame = (enabled: boolean): void => {
  try {
    getStorage()?.setItem(WINDOW_FRAME_STORAGE_KEY, String(enabled))
  } catch {
    // A storage failure must not prevent the current window from changing frame mode.
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
  useSystemWindowFrame: boolean
): WindowFrameMode => {
  const canChooseSystemFrame = platform.isTauri && (platform.isWindows || platform.isLinux)
  const usesSelectableCustomFrame = canChooseSystemFrame && !useSystemWindowFrame
  const hasCustomTitleBar = platform.isMac || !platform.isTauri || usesSelectableCustomFrame

  return {
    canChooseSystemFrame,
    hasCustomTitleBar,
    hasCustomWindowControls: usesSelectableCustomFrame,
    hasRoundedWindow: usesSelectableCustomFrame,
    searchInTitleBar:
      platform.isMac || ((platform.isWindows || platform.isLinux) && !useSystemWindowFrame),
  }
}

export const applyWindowFrameDocumentState = (hasRoundedWindow: boolean): void => {
  if (typeof document === 'undefined') return
  document.documentElement.dataset.ucCustomWindowFrame = String(hasRoundedWindow)
}
