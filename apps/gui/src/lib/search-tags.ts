import type { HistoryTagDto } from '@/api/daemon/history-tags'
import type { SearchTagDto } from '@/api/daemon/search'

export interface SearchTagOption {
  id: string
  count: number
  isBuiltin: boolean
  /** A custom tag's name from `GET /history/tags`; `null` when it cannot be
   * decrypted, absent while unknown. Builtin tags are named by i18n. */
  name?: string | null
}

// Mirror the backend's reserved builtin tag set (`link`/`code`/`favorited`/
// `image`/`directory`) so fallback options and builtin-first ordering stay consistent with
// what the server returns. Must match Engine `uc-core/src/search/tag.rs::builtin`: a
// search row's `tags` carries no builtin flag, so this set is what separates its
// builtin ids from local tag ids.
const BUILTIN_SEARCH_TAGS: SearchTagOption[] = [
  { id: 'link', count: 0, isBuiltin: true },
  { id: 'code', count: 0, isBuiltin: true },
  { id: 'favorited', count: 0, isBuiltin: true },
  { id: 'image', count: 0, isBuiltin: true },
  { id: 'directory', count: 0, isBuiltin: true },
]

const BUILTIN_TAG_IDS = new Set(BUILTIN_SEARCH_TAGS.map(tag => tag.id))

/** Whether `id` is a reserved builtin tag rather than a user's local tag. */
export function isBuiltinTagId(id: string): boolean {
  return BUILTIN_TAG_IDS.has(id)
}

/** The one display name of a tag, without the `#`: builtin tags by i18n, custom
 * tags by their name, a placeholder for a name that cannot be decrypted, and the
 * id itself while the name is unknown. */
export function tagLabel(
  tag: Pick<SearchTagOption, 'id' | 'name'> & { isBuiltin?: boolean },
  t: (key: string, options?: Record<string, unknown>) => string
): string {
  // Prefer the server's flag; the local set only covers a bare id.
  if (tag.isBuiltin ?? isBuiltinTagId(tag.id))
    return t(`history.type.${tag.id}`, { defaultValue: tag.id })
  if (tag.name === null) return t('history.tags.unreadable')
  return tag.name ?? tag.id
}

/** Attach the names of local history tags to the search tag options, and add
 * the local tags the search index has no entries for yet (a tag just created)
 * with a count of 0, so they can be picked and named like any other. */
export function withTagNames(
  options: SearchTagOption[],
  historyTags: HistoryTagDto[]
): SearchTagOption[] {
  if (historyTags.length === 0) return options
  const names = new Map(historyTags.map(tag => [tag.tagId, tag.name ?? null]))
  const indexed = new Set(options.map(option => option.id))
  return [
    ...options.map(option =>
      names.has(option.id) ? { ...option, name: names.get(option.id) } : option
    ),
    ...historyTags
      .filter(tag => !indexed.has(tag.tagId))
      .map(tag => ({ id: tag.tagId, count: 0, isBuiltin: false, name: tag.name ?? null })),
  ]
}

/** Split a tag selection (comma-separated, OR semantics on the search API). */
export function splitSearchTags(selection: string | null): string[] {
  return selection?.split(',').filter(Boolean) ?? []
}

/** Add `tag` to the selection, or remove it when already selected; empty → null. */
export function toggleSearchTag(selection: string | null, tag: string): string | null {
  const tags = splitSearchTags(selection)
  const next = tags.includes(tag) ? tags.filter(id => id !== tag) : [...tags, tag]
  return next.length > 0 ? next.join(',') : null
}

export function defaultSearchTagOptions(): SearchTagOption[] {
  return BUILTIN_SEARCH_TAGS
}

export function mergeSearchTagOptions(tags: SearchTagDto[]): SearchTagOption[] {
  const byId = new Map<string, SearchTagOption>()
  for (const tag of BUILTIN_SEARCH_TAGS) {
    byId.set(tag.id, tag)
  }
  for (const tag of tags) {
    byId.set(tag.tagId, {
      id: tag.tagId,
      count: tag.count,
      isBuiltin: tag.isBuiltin,
    })
  }
  return Array.from(byId.values()).sort((a: SearchTagOption, b: SearchTagOption) => {
    const aBuiltin = BUILTIN_SEARCH_TAGS.findIndex(tag => tag.id === a.id)
    const bBuiltin = BUILTIN_SEARCH_TAGS.findIndex(tag => tag.id === b.id)
    if (aBuiltin !== -1 || bBuiltin !== -1) {
      if (aBuiltin === -1) return 1
      if (bBuiltin === -1) return -1
      return aBuiltin - bBuiltin
    }
    return a.id.localeCompare(b.id)
  })
}
