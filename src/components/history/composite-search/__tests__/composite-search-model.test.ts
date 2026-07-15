import { Folder } from 'lucide-react'
import { describe, expect, it } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import {
  buildCandidates,
  parseBuffer,
  searchableTagsToOptions,
  type FilterSnapshot,
} from '../composite-search-model'

const t = (key: string) => key
const current: FilterSnapshot = {
  type: Filter.All,
  tag: null,
  source: null,
  time: 'all_time',
  extension: null,
}

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

  it('parses ext as a shared extension token', () => {
    expect(parseBuffer('ext:md')).toEqual({
      kind: 'token',
      dimension: 'extension',
      partial: 'md',
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

    expect(values).toEqual([Filter.Text, Filter.RichText, Filter.Image, Filter.File])
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

    expect(values).toEqual(['link', 'code', 'favorited', 'image', 'directory'])
  })

  it('offers the builtin directory tag as a folder', () => {
    const tagOptions = searchableTagsToOptions([])
    const [candidate] = buildCandidates('tag', 'directory', {
      t: key => (key === 'history.type.directory' ? '文件夹' : key),
      sourceOptions: [],
      current,
      tagOptions,
    })

    expect(candidate.value).toBe('directory')
    expect(candidate.label).toBe('文件夹')
    expect(candidate.icon).toBe(Folder)
  })
})
