import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ClipboardEntry, ClipboardEntryContent } from '@/lib/clipboard-entry'
import { formatRelativeTime, getItemPreview } from '../clipboard-utils'

function createEntry(
  type: ClipboardEntry['type'],
  content: ClipboardEntryContent | null
): ClipboardEntry {
  return {
    id: 'item-1',
    type,
    content,
    createdAt: 0,
    updatedAt: 0,
    activeTime: 0,
    isFavorited: false,
    isUnavailable: false,
  }
}

describe('clipboard-utils', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-03-16T00:00:00Z'))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('returns preview text for each supported entry type', () => {
    expect(
      getItemPreview(createEntry('text', { display_text: 'hello', has_detail: true, size: 5 }))
    ).toBe('hello')
    expect(
      getItemPreview(createEntry('image', { thumbnail: null, size: 1, width: 1, height: 1 }))
    ).toBe('Image | 1×1 | 1 B')
    expect(
      getItemPreview(createEntry('image', { thumbnail: null, size: 0, width: 0, height: 0 }))
    ).toBe('Image')
    expect(
      getItemPreview(createEntry('link', { urls: ['https://a.test'], domains: ['a.test'] }))
    ).toBe('https://a.test')
    expect(getItemPreview(createEntry('file', { file_names: ['a.txt'], file_sizes: [1] }))).toBe(
      'a.txt'
    )
    expect(getItemPreview(createEntry('code', { code: 'const x = 1' }))).toBe('const x = 1')
    expect(getItemPreview(createEntry('unknown', null))).toBe('')
  })

  it('formats relative time using quick-panel rules', () => {
    const now = Date.now()
    expect(formatRelativeTime(now)).toBe('just now')
    expect(formatRelativeTime(now - 5 * 60000)).toBe('5m')
    expect(formatRelativeTime(now - 2 * 3600000)).toBe('2h')
    expect(formatRelativeTime(now - 3 * 86400000)).toBe('3d')
  })
})
