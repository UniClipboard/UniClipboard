import { createLogger } from '@/lib/logger'

const log = createLogger('ui-scale')

export const UI_SCALE_STORAGE_KEY = 'uniclipboard.uiScale'
export const DEFAULT_UI_SCALE = 1
export const MIN_UI_SCALE = 0.8
export const MAX_UI_SCALE = 1.5
const UI_SCALE_CHANGED_EVENT = 'uniclipboard:ui-scale-changed'

export type UiScaleOption = {
  label: string
  value: number
}

export const UI_SCALE_OPTIONS: UiScaleOption[] = [
  { label: '80%', value: 0.8 },
  { label: '90%', value: 0.9 },
  { label: '100%', value: 1 },
  { label: '110%', value: 1.1 },
  { label: '125%', value: 1.25 },
  { label: '150%', value: 1.5 },
]

const roundUiScale = (value: number): number => Math.round(value * 100) / 100

export const clampUiScale = (value: number): number =>
  Math.min(MAX_UI_SCALE, Math.max(MIN_UI_SCALE, roundUiScale(value)))

const isTauriEnv = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

const getStorage = (storage?: Storage | null): Storage | null => {
  if (storage !== undefined) {
    return storage
  }

  if (typeof window === 'undefined') {
    return null
  }

  try {
    if (typeof window.localStorage === 'undefined') {
      return null
    }
    return window.localStorage
  } catch {
    return null
  }
}

const safeReadStorage = (storage: Storage | null, key: string): string | null => {
  if (!storage) {
    return null
  }

  try {
    return storage.getItem(key)
  } catch {
    return null
  }
}

const safeWriteStorage = (storage: Storage | null, key: string, value: string): void => {
  if (!storage) {
    return
  }

  try {
    storage.setItem(key, value)
  } catch {
    // Ignore storage write errors so UI scale changes never break startup or hotkeys.
  }
}

export const readStoredUiScale = (storage?: Storage | null): number => {
  const resolvedStorage = getStorage(storage)
  const raw = safeReadStorage(resolvedStorage, UI_SCALE_STORAGE_KEY)
  if (!raw) {
    return DEFAULT_UI_SCALE
  }

  const parsed = Number(raw)
  if (!Number.isFinite(parsed)) {
    return DEFAULT_UI_SCALE
  }

  return clampUiScale(parsed)
}

/**
 * Apply zoom via Tauri's native webview API to avoid coordinate mismatches
 * that CSS `zoom` causes with pointer-based libraries (resizable panels, context menus).
 */
export const applyUiScale = (scale: number): number => {
  const normalized = clampUiScale(scale)

  if (isTauriEnv()) {
    import('@tauri-apps/api/webview')
      .then(({ getCurrentWebview }) => {
        log.debug({ normalized }, 'calling setZoom')
        getCurrentWebview()
          .setZoom(normalized)
          .then(() => {
            log.debug('setZoom succeeded')
          })
          .catch(err => {
            log.error({ err }, 'setZoom failed')
          })
      })
      .catch(err => {
        log.error({ err }, 'import webview failed')
      })
  } else {
    log.debug('not in Tauri env, skipping setZoom')
  }

  return normalized
}

const persistUiScale = (scale: number, storage?: Storage | null): number => {
  const normalized = clampUiScale(scale)
  safeWriteStorage(getStorage(storage), UI_SCALE_STORAGE_KEY, String(normalized))
  return normalized
}

const dispatchUiScaleChanged = (): void => {
  if (typeof window === 'undefined') {
    return
  }
  window.dispatchEvent(new CustomEvent(UI_SCALE_CHANGED_EVENT))
}

export const subscribeUiScaleChanges = (listener: (scale: number) => void): (() => void) => {
  if (typeof window === 'undefined') {
    return () => {}
  }

  const handleCustomEvent = () => {
    listener(readStoredUiScale())
  }

  const handleStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === UI_SCALE_STORAGE_KEY) {
      listener(readStoredUiScale())
    }
  }

  window.addEventListener(UI_SCALE_CHANGED_EVENT, handleCustomEvent)
  window.addEventListener('storage', handleStorage)

  return () => {
    window.removeEventListener(UI_SCALE_CHANGED_EVENT, handleCustomEvent)
    window.removeEventListener('storage', handleStorage)
  }
}

export const setUiScale = (scale: number, storage?: Storage | null): number => {
  const normalized = applyUiScale(scale)
  persistUiScale(normalized, storage)
  dispatchUiScaleChanged()
  return normalized
}

export const adjustUiScale = (direction: 'in' | 'out', storage?: Storage | null): number => {
  const current = readStoredUiScale(storage)

  if (direction === 'in') {
    const next = UI_SCALE_OPTIONS.find(o => o.value > current)
    return next ? setUiScale(next.value, storage) : current
  } else {
    const prev = [...UI_SCALE_OPTIONS].reverse().find(o => o.value < current)
    return prev ? setUiScale(prev.value, storage) : current
  }
}

export const initializeUiScale = (storage?: Storage | null): (() => void) => {
  if (typeof window === 'undefined') {
    return () => {}
  }

  const syncFromStorage = () => {
    applyUiScale(readStoredUiScale(storage))
  }

  syncFromStorage()
  return subscribeUiScaleChanges(() => {
    syncFromStorage()
  })
}
