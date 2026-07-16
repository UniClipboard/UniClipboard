import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard'
import { playUiSound } from '@/lib/ui-sound'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/lib/ui-sound', () => ({ playUiSound: vi.fn() }))

const writeText = vi.fn()

describe('useCopyToClipboard', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
  })

  it('plays success feedback only after clipboard write succeeds', async () => {
    writeText.mockResolvedValueOnce(undefined)
    const { result } = renderHook(() => useCopyToClipboard())

    await act(async () => {
      expect(await result.current.copy('value')).toBe(true)
    })

    expect(playUiSound).toHaveBeenCalledWith('success')
  })

  it('keeps failed clipboard writes silent', async () => {
    writeText.mockRejectedValueOnce(new Error('denied'))
    const { result } = renderHook(() => useCopyToClipboard())

    await act(async () => {
      expect(await result.current.copy('value')).toBe(false)
    })

    expect(playUiSound).not.toHaveBeenCalled()
  })
})
