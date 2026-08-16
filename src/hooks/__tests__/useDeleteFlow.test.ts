import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useDeleteFlow } from '@/hooks/useDeleteFlow'

const preference = vi.hoisted(() => ({
  read: vi.fn(() => true),
  set: vi.fn(),
}))

vi.mock('@/lib/delete-confirmation-preference', () => ({
  readDeleteConfirmationEnabled: preference.read,
  setDeleteConfirmationEnabled: preference.set,
}))

describe('useDeleteFlow', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.clearAllMocks()
    preference.read.mockReturnValue(true)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('waits for confirmation and saves the do-not-ask choice before deleting', async () => {
    const remove = vi.fn().mockResolvedValue(undefined)
    const { result } = renderHook(() => useDeleteFlow(remove, 10))

    act(() => result.current.requestDelete('entry-1'))

    expect(result.current.deleteDialogOpen).toBe(true)
    expect(remove).not.toHaveBeenCalled()

    act(() => result.current.confirmDelete(true))
    expect(preference.set).toHaveBeenCalledWith(false)

    await act(async () => {
      await vi.runAllTimersAsync()
    })
    expect(remove).toHaveBeenCalledWith('entry-1')
  })

  it('deletes directly when confirmation is disabled', async () => {
    preference.read.mockReturnValue(false)
    const remove = vi.fn().mockResolvedValue(undefined)
    const { result } = renderHook(() => useDeleteFlow(remove, 10))

    act(() => result.current.requestDelete('entry-1'))

    expect(result.current.deleteDialogOpen).toBe(false)
    await act(async () => {
      await vi.runAllTimersAsync()
    })
    expect(remove).toHaveBeenCalledWith('entry-1')
  })
})
