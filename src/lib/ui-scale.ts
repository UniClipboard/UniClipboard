export const UI_SCALE_STORAGE_KEY = 'uniclipboard.uiScale'
export const DEFAULT_UI_SCALE = 1
export const MIN_UI_SCALE = 0.8
export const MAX_UI_SCALE = 1.5
export const UI_SCALE_STEP = 0.1
const UI_SCALE_CHANGED_EVENT = 'uniclipboard:ui-scale-changed'

const roundUiScale = (value: number): number => Math.round(value * 10) / 10

export const clampUiScale = (value: number): number =>
  Math.min(MAX_UI_SCALE, Math.max(MIN_UI_SCALE, roundUiScale(value)))

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

export const applyUiScale = (
  scale: number,
  root: HTMLElement | null = typeof document !== 'undefined' ? document.documentElement : null
): number => {
  const normalized = clampUiScale(scale)
  if (!root) {
    return normalized
  }

  if (normalized === DEFAULT_UI_SCALE) {
    root.style.removeProperty('zoom')
    root.style.removeProperty('--app-ui-scale')
    return normalized
  }

  root.style.setProperty('zoom', String(normalized))
  root.style.setProperty('--app-ui-scale', String(normalized))
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

export const setUiScale = (
  scale: number,
  root?: HTMLElement | null,
  storage?: Storage | null
): number => {
  const normalized = applyUiScale(scale, root)
  persistUiScale(normalized, storage)
  dispatchUiScaleChanged()
  return normalized
}

export const adjustUiScale = (
  direction: 'in' | 'out',
  root?: HTMLElement | null,
  storage?: Storage | null
): number => {
  const current = readStoredUiScale(storage)
  const delta = direction === 'in' ? UI_SCALE_STEP : -UI_SCALE_STEP
  return setUiScale(current + delta, root, storage)
}

export const initializeUiScale = (
  root: HTMLElement | null = typeof document !== 'undefined' ? document.documentElement : null,
  storage?: Storage | null
): (() => void) => {
  if (!root || typeof window === 'undefined') {
    return () => {}
  }

  const syncFromStorage = () => {
    applyUiScale(readStoredUiScale(storage), root)
  }

  const handleStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === UI_SCALE_STORAGE_KEY) {
      syncFromStorage()
    }
  }

  const handleCustomEvent = () => {
    syncFromStorage()
  }

  syncFromStorage()
  window.addEventListener('storage', handleStorage)
  window.addEventListener(UI_SCALE_CHANGED_EVENT, handleCustomEvent)

  return () => {
    window.removeEventListener('storage', handleStorage)
    window.removeEventListener(UI_SCALE_CHANGED_EVENT, handleCustomEvent)
  }
}
