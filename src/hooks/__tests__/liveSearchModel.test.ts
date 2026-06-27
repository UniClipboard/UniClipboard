import { describe, expect, it } from 'vitest'
import type { ClipboardEntryType, DisplayClipboardItem } from '@/lib/clipboard-entry'
import {
  canPatchLive,
  displayTypeToContentType,
  matchesFilter,
  MAX_LIVE_ITEMS,
  patchLiveItem,
  prependLiveItem,
  removeLiveItem,
  shouldRefetchOnSearchStatus,
  type LiveSearchQueryModel,
} from '../liveSearchModel'

function makeItem(
  partial: Partial<DisplayClipboardItem> & { id: string; type: ClipboardEntryType }
): DisplayClipboardItem {
  return { content: null, activeTime: 0, ...partial }
}

function model(partial: Partial<LiveSearchQueryModel> = {}): LiveSearchQueryModel {
  return { query: '', ...partial }
}

describe('displayTypeToContentType', () => {
  it('collapses display types back to physical content categories', () => {
    expect(displayTypeToContentType('text')).toBe('text')
    // A link is a text entry carrying web URLs, so it stays in the text category.
    expect(displayTypeToContentType('link')).toBe('text')
    expect(displayTypeToContentType('code')).toBe('html')
    expect(displayTypeToContentType('file')).toBe('file')
    expect(displayTypeToContentType('image')).toBe('image')
    expect(displayTypeToContentType('unknown')).toBe('other')
  })
})

describe('canPatchLive', () => {
  it('allows live patching for pure browse and content-type/tag filters', () => {
    expect(canPatchLive(model())).toBe(true)
    expect(canPatchLive(model({ contentTypes: 'text' }))).toBe(true)
    expect(canPatchLive(model({ tags: 'link' }))).toBe(true)
    expect(canPatchLive(model({ timeRange: 'all_time' }))).toBe(true)
  })

  it('refuses live patching when a non-judgeable dimension is active', () => {
    expect(canPatchLive(model({ query: 'foo' }))).toBe(false)
    expect(canPatchLive(model({ query: '   ' }))).toBe(true) // blank query == browse
    expect(canPatchLive(model({ sourceDevices: 'dev-1' }))).toBe(false)
    expect(canPatchLive(model({ extensions: 'pdf' }))).toBe(false)
    expect(canPatchLive(model({ timeRange: 'today' }))).toBe(false)
  })

  it('refuses live patching under the image tag (an image file is not client-judgeable)', () => {
    // A copied image file projects as a `file` display item, so the image tag
    // can't be judged client-side — defer to a server refetch.
    expect(canPatchLive(model({ tags: 'image' }))).toBe(false)
    expect(canPatchLive(model({ tags: 'link,image' }))).toBe(false)
    // Custom tags are likewise non-judgeable.
    expect(canPatchLive(model({ tags: 'project-x' }))).toBe(false)
    // The judgeable builtin tags still allow patching.
    expect(canPatchLive(model({ tags: 'link,favorited' }))).toBe(true)
  })
})

describe('matchesFilter', () => {
  it('matches everything under no filter', () => {
    expect(matchesFilter(makeItem({ id: 'a', type: 'text' }), model())).toBe(true)
    expect(matchesFilter(makeItem({ id: 'b', type: 'image' }), model())).toBe(true)
  })

  it('matches content-type against the physical category', () => {
    const m = model({ contentTypes: 'text' })
    expect(matchesFilter(makeItem({ id: 'a', type: 'text' }), m)).toBe(true)
    // link is physically text → the text filter includes it.
    expect(matchesFilter(makeItem({ id: 'b', type: 'link' }), m)).toBe(true)
    expect(matchesFilter(makeItem({ id: 'c', type: 'image' }), m)).toBe(false)
    expect(
      matchesFilter(makeItem({ id: 'd', type: 'code' }), model({ contentTypes: 'html' }))
    ).toBe(true)
  })

  it('OR-combines multiple content-types', () => {
    const m = model({ contentTypes: 'text,image' })
    expect(matchesFilter(makeItem({ id: 'a', type: 'image' }), m)).toBe(true)
    expect(matchesFilter(makeItem({ id: 'b', type: 'file' }), m)).toBe(false)
  })

  it('matches the link tag against the link display type', () => {
    const m = model({ tags: 'link' })
    expect(matchesFilter(makeItem({ id: 'a', type: 'link' }), m)).toBe(true)
    expect(matchesFilter(makeItem({ id: 'b', type: 'text' }), m)).toBe(false)
  })

  it('matches the favorited tag against the isFavorited flag', () => {
    const m = model({ tags: 'favorited' })
    expect(matchesFilter(makeItem({ id: 'a', type: 'text', isFavorited: true }), m)).toBe(true)
    expect(matchesFilter(makeItem({ id: 'b', type: 'text', isFavorited: false }), m)).toBe(false)
  })

  it('treats unknown/custom tags as non-matching', () => {
    const m = model({ tags: 'project-x' })
    expect(matchesFilter(makeItem({ id: 'a', type: 'text' }), m)).toBe(false)
  })
})

describe('prependLiveItem', () => {
  it('prepends a new entry', () => {
    const items = [makeItem({ id: 'a', type: 'text' })]
    const next = prependLiveItem(items, makeItem({ id: 'b', type: 'text' }))
    expect(next.map(it => it.id)).toEqual(['b', 'a'])
  })

  it('de-duplicates by id, floating the re-copy to the front', () => {
    const items = [
      makeItem({ id: 'a', type: 'text' }),
      makeItem({ id: 'b', type: 'text' }),
      makeItem({ id: 'c', type: 'text' }),
    ]
    const next = prependLiveItem(items, makeItem({ id: 'b', type: 'text', activeTime: 99 }))
    expect(next.map(it => it.id)).toEqual(['b', 'a', 'c'])
    expect(next[0].activeTime).toBe(99)
  })

  it('trims the oldest tail past the cap', () => {
    const items = [makeItem({ id: 'a', type: 'text' }), makeItem({ id: 'b', type: 'text' })]
    const next = prependLiveItem(items, makeItem({ id: 'c', type: 'text' }), 2)
    expect(next.map(it => it.id)).toEqual(['c', 'a'])
  })

  it('defaults the cap to MAX_LIVE_ITEMS', () => {
    const items = Array.from({ length: MAX_LIVE_ITEMS }, (_, i) =>
      makeItem({ id: `e${i}`, type: 'text' })
    )
    const next = prependLiveItem(items, makeItem({ id: 'new', type: 'text' }))
    expect(next).toHaveLength(MAX_LIVE_ITEMS)
    expect(next[0].id).toBe('new')
  })
})

describe('removeLiveItem', () => {
  it('drops the entry by id', () => {
    const items = [makeItem({ id: 'a', type: 'text' }), makeItem({ id: 'b', type: 'text' })]
    expect(removeLiveItem(items, 'a').map(it => it.id)).toEqual(['b'])
  })

  it('returns the same reference when the id is absent', () => {
    const items = [makeItem({ id: 'a', type: 'text' })]
    expect(removeLiveItem(items, 'missing')).toBe(items)
  })
})

describe('patchLiveItem', () => {
  it('merges the patch into the matching entry', () => {
    const items = [
      makeItem({ id: 'a', type: 'text', isFavorited: false }),
      makeItem({ id: 'b', type: 'text', isFavorited: false }),
    ]
    const next = patchLiveItem(items, 'b', { isFavorited: true })
    expect(next[0].isFavorited).toBe(false)
    expect(next[1].isFavorited).toBe(true)
  })

  it('returns the same reference when the id is absent', () => {
    const items = [makeItem({ id: 'a', type: 'text' })]
    expect(patchLiveItem(items, 'missing', { isFavorited: true })).toBe(items)
  })
})

describe('shouldRefetchOnSearchStatus', () => {
  it('refetches when the index becomes ready while showing the degraded view', () => {
    // The exact regression: a rebuild finishes (index → ready) while the browse
    // is degraded; without this refetch the banner would persist forever.
    expect(shouldRefetchOnSearchStatus({ state: 'ready' }, 'degraded')).toBe(true)
  })

  it('reads the legacy `status` key too (older daemon builds)', () => {
    expect(shouldRefetchOnSearchStatus({ status: 'ready' }, 'degraded')).toBe(true)
  })

  it('does not refetch when the current view is already ready', () => {
    // The index becoming ready while we are not degraded has nothing to clear.
    expect(shouldRefetchOnSearchStatus({ state: 'ready' }, 'ready')).toBe(false)
  })

  it('does not refetch on non-ready status updates', () => {
    expect(shouldRefetchOnSearchStatus({ state: 'rebuilding' }, 'degraded')).toBe(false)
    expect(shouldRefetchOnSearchStatus({ state: 'unavailable' }, 'degraded')).toBe(false)
    expect(shouldRefetchOnSearchStatus(undefined, 'degraded')).toBe(false)
    expect(shouldRefetchOnSearchStatus({}, 'degraded')).toBe(false)
  })
})
