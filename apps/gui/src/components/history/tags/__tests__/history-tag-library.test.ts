import { describe, expect, it } from 'vitest'
import type { HistoryTagDto } from '@/api/daemon/history-tags'
import { libraryTags, similarTagOf, sortTags } from '../history-tag-library'

function tag(tagId: string, name: string | null, entryCount = 0, createdAtMs = 0): HistoryTagDto {
  return { tagId, name, entryCount, createdAtMs }
}

describe('sortTags', () => {
  const tags = [tag('a', 'work', 5, 1), tag('b', 'deploy', 9, 3), tag('c', null, 9, 2)]

  it('orders by use, then by name with unreadable names last', () => {
    expect(sortTags(tags, 'mostUsed').map(t => t.tagId)).toEqual(['b', 'c', 'a'])
  })

  it('orders by name', () => {
    expect(sortTags(tags, 'name').map(t => t.tagId)).toEqual(['b', 'a', 'c'])
  })

  it('orders newest first', () => {
    expect(sortTags(tags, 'newest').map(t => t.tagId)).toEqual(['b', 'c', 'a'])
  })
})

describe('similarTagOf', () => {
  it('finds a tag that differs by case, spacing, a suffix or a typo', () => {
    const tags = [
      tag('1', 'Release', 2),
      tag('2', 'releases', 12),
      tag('3', 'hot-fix', 1),
      tag('4', 'hotfix', 4),
      tag('5', 'docker', 30),
      tag('6', 'dockr', 1),
    ]
    expect(similarTagOf(tags[0], tags)?.tagId).toBe('2')
    expect(similarTagOf(tags[2], tags)?.tagId).toBe('4')
    expect(similarTagOf(tags[5], tags)?.tagId).toBe('5')
  })

  it('suggests the most used of several look-alikes', () => {
    const tags = [tag('1', 'deploy', 1), tag('2', 'deploys', 3), tag('3', 'Deploy', 20)]
    expect(similarTagOf(tags[0], tags)?.tagId).toBe('3')
  })

  it('leaves short, different and unreadable names alone', () => {
    const tags = [tag('1', 'ui', 3), tag('2', 'uk', 5), tag('3', 'work', 1), tag('4', null, 2)]
    expect(similarTagOf(tags[0], tags)).toBeNull()
    expect(similarTagOf(tags[2], tags)).toBeNull()
    expect(similarTagOf(tags[3], tags)).toBeNull()
    const stems = [
      tag('1', 'git', 3),
      tag('2', 'github', 5),
      tag('3', 'device', 2),
      tag('4', 'dev', 1),
    ]
    expect(similarTagOf(stems[0], stems)).toBeNull()
    expect(similarTagOf(stems[3], stems)).toBeNull()
  })
})

describe('libraryTags', () => {
  it('lists the four builtin tags first, counted by the search index, then local tags', () => {
    const rows = libraryTags(
      [tag('a', 'work', 5, 1)],
      [
        { id: 'link', count: 3, isBuiltin: true },
        { id: 'favorited', count: 9, isBuiltin: true },
      ],
      id => `label:${id}`
    )
    expect(rows.map(row => [row.tagId, row.name, row.entryCount, row.builtin])).toEqual([
      ['link', 'label:link', 3, true],
      ['code', 'label:code', 0, true],
      ['image', 'label:image', 0, true],
      ['directory', 'label:directory', 0, true],
      ['a', 'work', 5, false],
    ])
  })
})
