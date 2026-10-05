import type { HistoryTagDto } from '@/api/daemon/history-tags'
import { similarNames } from '@/components/history/tags/history-tag-library'

/** Longest tag name the Engine accepts (Unicode scalars, after trimming). */
export const MAX_TAG_NAME_LENGTH = 64

const MAX_SUGGESTIONS = 6
/** Most used tags offered when none resembles the typed name. */
const MAX_FREQUENT = 3

/** `similar`: resembles the typed name; `frequent`: offered because nothing
 * does (or nothing is typed), most used first. */
export type TagSuggestionGroup = 'similar' | 'frequent'

export type TagSuggestion =
  | { kind: 'create'; name: string }
  | { kind: 'existing'; tag: HistoryTagDto; group: TagSuggestionGroup }

/**
 * What the tag editor offers for `query` (HDetail.dc.html `tagging`): "Create
 * #query" first when no tag already has that name (ignoring case), then the
 * tags that resemble it — starting with it, containing it, or a typo apart —
 * so a near-duplicate is one keystroke away. When none does, or nothing is
 * typed, the most used tags instead. Tags already on the entry, and tags
 * whose name cannot be read, are left out.
 */
export function tagSuggestions(
  query: string,
  tags: HistoryTagDto[],
  attachedIds: ReadonlySet<string>
): TagSuggestion[] {
  const name = query.trim()
  const needle = name.toLocaleLowerCase()
  const named = tags.flatMap(tag => (tag.name ? [{ tag, key: tag.name.toLocaleLowerCase() }] : []))
  const candidates = named.filter(({ tag }) => !attachedIds.has(tag.tagId))
  const byUse = (a: { tag: HistoryTagDto }, b: { tag: HistoryTagDto }) =>
    b.tag.entryCount - a.tag.entryCount
  const similar =
    needle === ''
      ? []
      : candidates
          .flatMap(({ tag, key }) => {
            if (key.startsWith(needle)) return [{ tag, rank: 0 }]
            if (key.includes(needle)) return [{ tag, rank: 1 }]
            return similarNames(key, needle) ? [{ tag, rank: 2 }] : []
          })
          .sort((a, b) => a.rank - b.rank || byUse(a, b))
          .slice(0, MAX_SUGGESTIONS)
          .map(({ tag }): TagSuggestion => ({ kind: 'existing', tag, group: 'similar' }))
  const existing =
    similar.length > 0
      ? similar
      : [...candidates]
          .sort(byUse)
          .slice(0, needle === '' ? MAX_SUGGESTIONS : MAX_FREQUENT)
          .map(({ tag }): TagSuggestion => ({ kind: 'existing', tag, group: 'frequent' }))
  const exists = named.some(({ key }) => key === needle)
  return name === '' || exists ? existing : [{ kind: 'create', name }, ...existing]
}
