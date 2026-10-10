import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useLiveSearch } from '@/hooks/useLiveSearch'
import type { ClipboardEntry } from '@/lib/clipboard-entry'

const stream = vi.hoisted(() => ({ onLocalItem: null as null | ((entry: ClipboardEntry) => void) }))
const summarizeEntryTags = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useClipboardEventStream', () => ({
  useClipboardEventStream: (options: { onLocalItem: (entry: ClipboardEntry) => void }) => {
    stream.onLocalItem = options.onLocalItem
  },
}))
vi.mock('@/hooks/useEncryptionSessionState', () => ({
  useEncryptionSessionState: () => ({ encryptionReady: true, isLocked: false }),
}))
vi.mock('@/api/daemon/search', () => ({
  querySearch: vi.fn().mockResolvedValue({
    data: { items: [], total: 0, hasMore: false, state: 'ready' },
  }),
}))
vi.mock('@/api/daemon/history-tags', () => ({ summarizeEntryTags }))
vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: { subscribe: () => () => {}, onReconnect: () => () => {} },
}))

const entry = {
  id: 'entry-new',
  type: 'text',
  content: { display_text: 'docker compose up -d', has_detail: false, size: 20, char_count: 20 },
  activeTime: 1,
} as unknown as ClipboardEntry

describe('useLiveSearch local entries', () => {
  beforeEach(() => summarizeEntryTags.mockReset())

  it("fills in a new entry's local tags once they are known", async () => {
    summarizeEntryTags.mockResolvedValue({
      selected: 1,
      tags: [{ tagId: 't-deploy', applied: 1 }],
    })
    const { result } = renderHook(() =>
      useLiveSearch({ model: { query: '', timeRange: 'all_time' } })
    )
    await waitFor(() => expect(result.current.isLoading).toBe(false))

    act(() => stream.onLocalItem?.(entry))
    expect(result.current.items[0].id).toBe('entry-new')
    await waitFor(() => expect(result.current.items[0].userTagIds).toEqual(['t-deploy']))
    expect(summarizeEntryTags).toHaveBeenCalledWith(['entry-new'])
  })

  it('records a new entry without tags as having none', async () => {
    summarizeEntryTags.mockResolvedValue({ selected: 1, tags: [] })
    const { result } = renderHook(() =>
      useLiveSearch({ model: { query: '', timeRange: 'all_time' } })
    )
    await waitFor(() => expect(result.current.isLoading).toBe(false))

    act(() => stream.onLocalItem?.(entry))
    await waitFor(() => expect(result.current.items[0].userTagIds).toEqual([]))
  })
})
