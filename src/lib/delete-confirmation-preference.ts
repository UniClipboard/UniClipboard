export const DELETE_CONFIRMATION_STORAGE_KEY = 'uniclipboard.confirmBeforeDeletingHistoryItem'
export const DEFAULT_DELETE_CONFIRMATION_ENABLED = true

const resolveStorage = (storage?: Storage | null): Storage | null => {
  if (storage !== undefined) return storage
  if (typeof window === 'undefined') return null

  try {
    return window.localStorage
  } catch {
    return null
  }
}

export function readDeleteConfirmationEnabled(storage?: Storage | null): boolean {
  try {
    const value = resolveStorage(storage)?.getItem(DELETE_CONFIRMATION_STORAGE_KEY)
    if (value === 'false') return false
    return DEFAULT_DELETE_CONFIRMATION_ENABLED
  } catch {
    return DEFAULT_DELETE_CONFIRMATION_ENABLED
  }
}

export function setDeleteConfirmationEnabled(enabled: boolean, storage?: Storage | null): void {
  try {
    resolveStorage(storage)?.setItem(DELETE_CONFIRMATION_STORAGE_KEY, String(enabled))
  } catch {
    // A storage failure must not block deletion or settings navigation.
  }
}
