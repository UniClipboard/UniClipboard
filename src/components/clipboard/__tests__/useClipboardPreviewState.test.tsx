import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { DisplayClipboardItem } from '../ClipboardContent'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

const useAppSelectorMock = vi.fn()
const cacheGetMock = vi.fn()

vi.mock('@/store/hooks', () => ({
  useAppSelector: (selector: (state: unknown) => unknown) => useAppSelectorMock(selector),
}))

vi.mock('@/store/slices/fileTransferSlice', () => ({
  resolveEntryTransferStatus: vi.fn(() => 'completed'),
  selectEntryTransferStatus: vi.fn(() => undefined),
  selectTransferByEntryId: vi.fn(() => undefined),
  selectTransferByTransferIds: vi.fn(() => undefined),
}))

vi.mock('@/lib/clipboard-preview-cache', () => ({
  clipboardPreviewCache: {
    get: (...args: unknown[]) => cacheGetMock(...args),
  },
}))

function createFileItem(overrides: Partial<DisplayClipboardItem> = {}): DisplayClipboardItem {
  return {
    id: 'entry-file',
    type: 'file',
    time: 'just now',
    activeTime: Date.now(),
    content: {
      file_names: ['uniclipboard-aarch64-apple-darwin.zip'],
      file_sizes: [64 * 1024 * 1024],
    },
    ...overrides,
  }
}

describe('useClipboardPreviewState', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useAppSelectorMock.mockImplementation((selector: (state: unknown) => unknown) => selector({}))
  })

  it('loads preview data for previewable items and exposes the resolved status', async () => {
    const item = createFileItem()
    cacheGetMock.mockResolvedValue({
      entryId: 'entry-file',
      contentType: 'file',
      sizeBytes: 64 * 1024 * 1024,
      fileNames: ['uniclipboard-aarch64-apple-darwin.zip'],
    } satisfies ClipboardPreviewData)

    const { result } = renderHook(() => useClipboardPreviewState(item))

    expect(result.current.loading).toBe(true)
    expect(result.current.preview).toBeNull()
    expect(result.current.effectiveStatus).toBe('completed')

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
      expect(result.current.preview?.entryId).toBe('entry-file')
    })
  })

  it('stays idle when there is no selected item', () => {
    const { result } = renderHook(() => useClipboardPreviewState(null))

    expect(result.current.loading).toBe(false)
    expect(result.current.preview).toBeNull()
    expect(cacheGetMock).not.toHaveBeenCalled()
  })
})
