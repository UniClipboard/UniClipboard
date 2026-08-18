import { Folder } from 'lucide-react'
import { describe, expect, it } from 'vitest'
import { Filter } from '@/api/clipboardItems'
import {
  buildCandidates,
  buildChips,
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

  it('ranks unfiltered tags by the number of matching records', () => {
    const values = buildCandidates('tag', '', {
      t,
      sourceOptions: [],
      current,
      tagOptions: [
        { id: 'link', count: 2, isBuiltin: true },
        { id: 'code', count: 8, isBuiltin: true },
        { id: 'project', count: 5, isBuiltin: false },
      ],
    }).map(candidate => candidate.value)

    expect(values).toEqual(['code', 'project', 'link'])
  })

  it('ranks exact and prefix tag matches ahead of more frequent partial matches', () => {
    const values = buildCandidates('tag', 'code', {
      t,
      sourceOptions: [],
      current,
      tagOptions: [
        { id: 'archive-code', count: 20, isBuiltin: false },
        { id: 'codec', count: 5, isBuiltin: false },
        { id: 'code', count: 1, isBuiltin: true },
      ],
    }).map(candidate => candidate.value)

    expect(values).toEqual(['code', 'codec', 'archive-code'])
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

  it('prefixes a selected tag chip with a hash while preserving its localized name', () => {
    const [chip] = buildChips({
      t: key => (key === 'history.type.link' ? '链接' : key),
      sourceOptions: [],
      tagOptions: [{ id: 'link', count: 2, isBuiltin: true }],
      current: { ...current, tag: 'link' },
    })

    expect(chip.dimension).toBe('tag')
    expect(chip.label).toBe('#链接')
  })
})
