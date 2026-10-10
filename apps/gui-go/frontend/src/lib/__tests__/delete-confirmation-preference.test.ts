import { beforeEach, describe, expect, it } from 'vitest'
import {
  DELETE_CONFIRMATION_STORAGE_KEY,
  readDeleteConfirmationEnabled,
  setDeleteConfirmationEnabled,
} from '@/lib/delete-confirmation-preference'

describe('delete confirmation preference', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('keeps confirmation enabled when an existing profile has no saved preference', () => {
    expect(readDeleteConfirmationEnabled()).toBe(true)
  })

  it('persists a disabled preference across reads', () => {
    setDeleteConfirmationEnabled(false)

    expect(localStorage.getItem(DELETE_CONFIRMATION_STORAGE_KEY)).toBe('false')
    expect(readDeleteConfirmationEnabled()).toBe(false)
  })
})
