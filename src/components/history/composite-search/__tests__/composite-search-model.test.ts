import { describe, expect, it } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import {
  buildCandidates,
  parseBuffer,
  searchableTagsToOptions,
  type FilterSnapshot,
} from '../composite-search-model'

const t = (key: string) => key
const current: FilterSnapshot = { type: Filter.All, tag: null, source: null, time: 'all_time' }

describe('composite search model', () => {
  it('parses # as a tag token', () => {
    expect(parseBuffer('#')).toEqual({
      kind: 'token',
      dimension: 'tag',
      partial: '',
      committed: false,
    })
    expect(parseBuffer('#lin')).toEqual({
      kind: 'token',
      dimension: 'tag',
      partial: 'lin',
      committed: false,
    })
  })

  it('offers only physical content types under type', () => {
    const values = buildCandidates('type', '', {
      t,
      sourceOptions: [],
      current,
      tagOptions: [],
    }).map(c => c.value)

    expect(values).toEqual([Filter.Text, Filter.Image, Filter.File])
  })

  it('converts searchable tags into tag candidates', () => {
    const tagOptions = searchableTagsToOptions([
      { tagId: 'link', count: 2, isBuiltin: true },
      { tagId: 'code', count: 1, isBuiltin: true },
    ])
    const values = buildCandidates('tag', '', {
      t,
      sourceOptions: [],
      current,
      tagOptions,
    }).map(c => c.value)

    expect(values).toEqual(['link', 'code', 'favorited', 'image'])
  })
})
