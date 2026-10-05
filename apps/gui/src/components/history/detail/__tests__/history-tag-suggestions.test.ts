import { describe, expect, it } from 'vitest'
import type { HistoryTagDto } from '@/api/daemon/history-tags'
import { tagSuggestions } from '../history-tag-suggestions'

const tag = (tagId: string, name: string, entryCount: number): HistoryTagDto => ({
  tagId,
  name,
  entryCount,
  createdAtMs: 0,
})
const tags = [tag('r', 'release', 12), tag('i', 'incident', 4), tag('d', 'docker', 38)]
const summary = (query: string, attached: string[] = []) =>
  tagSuggestions(query, tags, new Set(attached)).map(s =>
    s.kind === 'create' ? `create:${s.name}` : `${s.group}:${s.tag.name}`
  )

describe('tagSuggestions', () => {
  it('offers to create a new name, then the tags that resemble it', () => {
    expect(summary('releas')).toEqual(['create:releas', 'similar:release'])
    expect(summary('dockr')).toEqual(['create:dockr', 'similar:docker'])
  })

  it('falls back to the three most used tags when none resembles the name', () => {
    expect(summary('hotfix')).toEqual([
      'create:hotfix',
      'frequent:docker',
      'frequent:release',
      'frequent:incident',
    ])
  })

  it('offers no create for an existing name and leaves attached tags out', () => {
    expect(summary('Docker')).toEqual(['similar:docker'])
    expect(summary('', ['d'])).toEqual(['frequent:release', 'frequent:incident'])
  })
})
