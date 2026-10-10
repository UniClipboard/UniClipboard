import { describe, expect, it } from 'vitest'
import type { LiveSearchQueryModel } from '@/hooks/liveSearchModel'
import { growQueryWindow, resolveQueryWindowLimit } from '@/hooks/useLiveSearch'

const browse: LiveSearchQueryModel = { query: '', timeRange: 'all_time' }
const search: LiveSearchQueryModel = { query: 'needle', timeRange: 'all_time' }

describe('live-search query window', () => {
  it('uses the grown limit while the query model is unchanged', () => {
    expect(resolveQueryWindowLimit({ model: browse, pageSize: 100, limit: 300 }, browse, 100)).toBe(
      300
    )
  })

  it('uses one page immediately when the query model changes', () => {
    expect(resolveQueryWindowLimit({ model: browse, pageSize: 100, limit: 300 }, search, 100)).toBe(
      100
    )
  })

  it('grows from one page after a query model change', () => {
    expect(growQueryWindow({ model: browse, pageSize: 100, limit: 300 }, search, 100)).toEqual({
      model: search,
      pageSize: 100,
      limit: 200,
    })
  })
})
