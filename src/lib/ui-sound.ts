import { bind, play, setEnabled, type SoundName } from 'cuelume'
import { createLogger } from '@/lib/logger'

const log = createLogger('ui-sound')

export const UI_SOUND_STORAGE_KEY = 'uniclipboard.uiSoundEnabled'
export const DEFAULT_UI_SOUND_ENABLED = true
const UI_SOUND_CHANGED_EVENT = 'uniclipboard:ui-sound-changed'

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

export const readStoredUiSoundEnabled = (storage?: Storage | null): boolean => {
  const resolvedStorage = getStorage(storage)
  if (!resolvedStorage) {
    return DEFAULT_UI_SOUND_ENABLED
  }

  try {
    const raw = resolvedStorage.getItem(UI_SOUND_STORAGE_KEY)
    if (raw === null) {
      return DEFAULT_UI_SOUND_ENABLED
    }
    return raw === 'true'
  } catch {
    return DEFAULT_UI_SOUND_ENABLED
  }
}

const dispatchUiSoundChanged = (): void => {
  if (typeof window === 'undefined') {
    return
  }
  window.dispatchEvent(new CustomEvent(UI_SOUND_CHANGED_EVENT))
}

export const subscribeUiSoundChanges = (listener: (enabled: boolean) => void): (() => void) => {
  if (typeof window === 'undefined') {
    return () => {}
  }

  const handleCustomEvent = () => {
    listener(readStoredUiSoundEnabled())
  }

  const handleStorage = (event: StorageEvent) => {
    if (event.key === null || event.key === UI_SOUND_STORAGE_KEY) {
      listener(readStoredUiSoundEnabled())
    }
  }

  window.addEventListener(UI_SOUND_CHANGED_EVENT, handleCustomEvent)
  window.addEventListener('storage', handleStorage)

  return () => {
    window.removeEventListener(UI_SOUND_CHANGED_EVENT, handleCustomEvent)
    window.removeEventListener('storage', handleStorage)
  }
}

export const setUiSoundEnabled = (enabled: boolean, storage?: Storage | null): void => {
  const resolvedStorage = getStorage(storage)
  try {
    resolvedStorage?.setItem(UI_SOUND_STORAGE_KEY, String(enabled))
  } catch {
    // Ignore storage write errors so the toggle never breaks the settings UI.
  }
  setEnabled(enabled)
  dispatchUiSoundChanged()
}

/**
 * Plays a semantic UI cue while keeping Cuelume ownership in one module.
 *
 * Playback is best-effort: callers invoke this inside the success path of
 * clipboard/paste flows, so a throwing `play()` would reach their `catch` and
 * report a completed operation as failed. Swallow the error to keep that
 * guarantee local rather than borrowing it from Cuelume's internals.
 */
export const playUiSound = (sound: SoundName): void => {
  try {
    play(sound)
  } catch (err) {
    log.warn({ err }, 'Failed to play UI sound')
  }
}

/**
 * Wires up cuelume's `data-cuelume-*` delegated listeners once and syncs its
 * global enabled flag with the persisted preference. Safe to call repeatedly.
 */
export const initializeUiSound = (storage?: Storage | null): (() => void) => {
  if (typeof window === 'undefined') {
    return () => {}
  }

  bind()
  setEnabled(readStoredUiSoundEnabled(storage))

  return subscribeUiSoundChanges(enabled => {
    setEnabled(enabled)
  })
}
