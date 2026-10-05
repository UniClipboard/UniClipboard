import type { SearchTagDto } from '@/api/daemon/search'

export interface SearchTagOption {
  id: string
  count: number
  isBuiltin: boolean
}

// Mirror the backend's reserved builtin tag set (`link`/`code`/`favorited`/
// `image`/`directory`) so fallback options and builtin-first ordering stay consistent with
// what the server returns.
const BUILTIN_SEARCH_TAGS: SearchTagOption[] = [
  { id: 'link', count: 0, isBuiltin: true },
  { id: 'code', count: 0, isBuiltin: true },
  { id: 'favorited', count: 0, isBuiltin: true },
  { id: 'image', count: 0, isBuiltin: true },
  { id: 'directory', count: 0, isBuiltin: true },
]

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
