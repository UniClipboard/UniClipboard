import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { formatRelativeTime, getItemPreview, resolveItemType } from '../clipboard-utils'
import type { ClipboardItemResponse } from '@/api/clipboardItems'

function createItemResponse(
  partial: Partial<ClipboardItemResponse['item']>
): ClipboardItemResponse {
  return {
    id: 'item-1',
    is_downloaded: true,
    is_favorited: false,
    created_at: 0,
    updated_at: 0,
    active_time: 0,
    item: {
      text: null,
      image: null,
      file: null,
      link: null,
      code: null,
      unknown: null,
      ...partial,
    },
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

  it('resolves item type by payload shape', () => {
    expect(
      resolveItemType(
        createItemResponse({ text: { display_text: 'hello', has_detail: true, size: 5 } })
      )
    ).toBe('text')
    expect(
      resolveItemType(
        createItemResponse({ image: { thumbnail: null, size: 1, width: 1, height: 1 } })
      )
    ).toBe('image')
    expect(
      resolveItemType(
        createItemResponse({ link: { urls: ['https://a.test'], domains: ['a.test'] } })
      )
    ).toBe('link')
    expect(
      resolveItemType(createItemResponse({ file: { file_names: ['a.txt'], file_sizes: [1] } }))
    ).toBe('file')
    expect(resolveItemType(createItemResponse({ code: { code: 'const x = 1' } }))).toBe('code')
    expect(resolveItemType(createItemResponse({}))).toBe('unknown')
  })

  it('returns preview text for each supported item type', () => {
    expect(
      getItemPreview(
        createItemResponse({ text: { display_text: 'hello', has_detail: true, size: 5 } })
      )
    ).toBe('hello')
    expect(
      getItemPreview(
        createItemResponse({ image: { thumbnail: null, size: 1, width: 1, height: 1 } })
      )
    ).toBe('Image')
    expect(
      getItemPreview(
        createItemResponse({ link: { urls: ['https://a.test'], domains: ['a.test'] } })
      )
    ).toBe('https://a.test')
    expect(
      getItemPreview(createItemResponse({ file: { file_names: ['a.txt'], file_sizes: [1] } }))
    ).toBe('a.txt')
    expect(getItemPreview(createItemResponse({ code: { code: 'const x = 1' } }))).toBe(
      'const x = 1'
    )
  })

  it('formats relative time using quick-panel rules', () => {
    const now = Date.now()
    expect(formatRelativeTime(now)).toBe('just now')
    expect(formatRelativeTime(now - 5 * 60000)).toBe('5m')
    expect(formatRelativeTime(now - 2 * 3600000)).toBe('2h')
    expect(formatRelativeTime(now - 3 * 86400000)).toBe('3d')
  })
})
