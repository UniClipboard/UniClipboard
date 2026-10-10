import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { makeBaseSettings } from '@/test/fixtures/settings'
import { useFileSyncNumbers } from '../useFileSyncNumbers'

describe('file sync numeric editing', () => {
  it('retains a newer identical draft when an older save completes', async () => {
    let finish!: () => void
    const save = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise<void>(resolve => {
            finish = resolve
          })
      )
      .mockImplementation(() => new Promise(() => {}))
    const { result } = renderHook(() => useFileSyncNumbers(makeBaseSettings().fileSync, save))
    act(() => result.current.change('smallFileThreshold', '20'))
    act(() => result.current.change('smallFileThreshold', '30'))
    act(() => result.current.change('smallFileThreshold', '20'))
    await act(async () => finish())
    expect(result.current.fields.smallFileThreshold.value).toBe('20')
  })

  it('rechecks both size limits when either draft changes', () => {
    const save = vi.fn(() => new Promise<void>(() => {}))
    const { result } = renderHook(() => useFileSyncNumbers(makeBaseSettings().fileSync, save))
    act(() => result.current.change('maxFileSize', '5'))
    expect(result.current.fields.maxFileSize.error).toContain('exceedsMax')
    act(() => result.current.change('smallFileThreshold', '2'))
    expect(result.current.fields.maxFileSize.error).toBeNull()
    expect(save).toHaveBeenLastCalledWith({
      smallFileThreshold: 2 * 1024 * 1024,
      maxFileSize: 5 * 1024 * 1024,
    })
  })

  it('does not save empty or invalid values and restores persisted values on failure', async () => {
    const save = vi.fn().mockRejectedValue(new Error('offline'))
    const { result } = renderHook(() => useFileSyncNumbers(makeBaseSettings().fileSync, save))
    act(() => result.current.change('fileRetentionHours', ''))
    act(() => result.current.change('fileRetentionHours', '1.5'))
    act(() => result.current.change('fileRetentionHours', '721'))
    expect(save).not.toHaveBeenCalled()
    await act(async () => result.current.change('fileRetentionHours', '48'))
    expect(result.current.fields.fileRetentionHours.value).toBe('24')
  })
})
