import { renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { clearHistorySessionSnapshot } from '@/hooks/historySessionSnapshot'
import { useHistoryData } from '@/hooks/useHistoryData'
import { clipboardPreviewCache } from '@/lib/clipboard-preview-cache'

const fixture = vi.hoisted(() => ({
  offset: 0,
  items: Array.from({ length: 50 }, (_, i) => ({
    id: `real-${i}`,
    type: 'text',
    activeTime: 0,
    content: null,
  })),
  pending: [0, 1].map(i => ({
    entryId: `pending-${i}`,
    fromDevice: 'peer',
    createdAt: 0,
    filenames: ['incoming'],
    totalBytes: 1,
  })),
  reportViewport: vi.fn(),
  ensureItem: vi.fn(),
  ensureNeighbor: vi.fn(),
  moveToTop: vi.fn(),
}))
vi.mock('@/store/hooks', () => ({
  useAppSelector: (selector: (state: unknown) => unknown) =>
    selector({
      clipboard: { pendingItems: fixture.pending },
      devices: { spaceMembers: [], localDevice: null },
    }),
}))
vi.mock('@/hooks/useHistorySourceOptions', () => ({ useHistorySourceOptions: () => [] }))
vi.mock('@/hooks/useHistoryWindowImages', () => ({ useHistoryWindowImages: vi.fn() }))
vi.mock('@/hooks/useHistoryWindow', () => ({
  useHistoryWindow: () => ({
    items: fixture.items,
    offset: fixture.offset,
    total: 200,
    state: 'ready',
    hasMore: true,
    reportViewport: fixture.reportViewport,
    ensureItem: fixture.ensureItem,
    ensureNeighbor: fixture.ensureNeighbor,
    moveToTop: fixture.moveToTop,
    isAtTop: () => fixture.offset === 0,
    resultKey: 'browse',
    resultsReady: true,
    headRevision: 0,
    isLoading: false,
    failure: null,
    growWindow: vi.fn(),
    navigate: vi.fn(),
    removeItem: vi.fn(),
    refetch: vi.fn(),
  }),
}))

describe('history pending-row coordinates', () => {
  beforeEach(() => {
    clearHistorySessionSnapshot()
    fixture.offset = 0
    vi.clearAllMocks()
  })
  it('caps rendered rows, retains the selected preview and translates head indices to engine indices', async () => {
    const retain = vi.spyOn(clipboardPreviewCache, 'retain')
    const { result } = renderHook(() => useHistoryData('selected-outside'))
    expect(result.current.baseItems).toHaveLength(50)
    expect(result.current.viewport.total).toBe(202)
    expect(result.current.baseItems[0].id).toBe('pending-0')
    result.current.viewport.onVisibleRange(12, 15, true)
    expect(fixture.reportViewport).toHaveBeenCalledWith(10, 13, true)
    expect(retain).toHaveBeenCalledWith(
      new Set(result.current.baseItems.map(item => item.id)),
      'selected-outside'
    )
    await result.current.ensureItem(2)
    expect(fixture.ensureItem).toHaveBeenCalledWith(0)
    retain.mockRestore()
  })
  it('keeps pending slots in the global top spacer while only rendering engine records in a deep window', async () => {
    fixture.offset = 30
    const { result } = renderHook(() => useHistoryData())
    expect(result.current.baseItems).toBe(fixture.items)
    expect(result.current.viewport.offset).toBe(32)
    expect(result.current.viewport.total).toBe(202)
    result.current.viewport.onVisibleRange(5, 9, true)
    expect(fixture.reportViewport).toHaveBeenCalledWith(5, 9, true)
    await result.current.ensureItem(35)
    expect(fixture.ensureItem).toHaveBeenCalledWith(33)
    fixture.ensureNeighbor.mockResolvedValueOnce({ item: fixture.items[5], index: 35 })
    const neighbor = await result.current.ensureNeighbor('real-4', 36, 1)
    expect(fixture.ensureNeighbor).toHaveBeenCalledWith('real-4', 34, 1)
    expect(neighbor?.index).toBe(37)
  })
})
