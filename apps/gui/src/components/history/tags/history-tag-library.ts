import type { HistoryTagDto } from '@/api/daemon/history-tags'
import type { SearchTagOption } from '@/lib/search-tags'

/** Builtin tags the Library lists and the sidebar can hold, in the daemon's
 * default sidebar order (mirrors `BUILTINS` in `uc-webserver`'s tag layout).
 * `favorited` is the Pinned row, so it is not among them. */
export const LIBRARY_BUILTIN_TAG_IDS = ['link', 'code', 'image', 'directory'] as const

/** A Library row: a local tag, or a builtin tag (named by i18n, counted by
 * the search index, never renamed, merged or deleted). */
export interface LibraryTag {
  tagId: string
  /** A local tag's name (`null`: unreadable); a builtin tag's label. */
  name: string | null
  entryCount: number
  createdAtMs: number
  builtin: boolean
}

/** The Library's rows: the builtin tags, then this device's local tags. */
export function libraryTags(
  local: HistoryTagDto[],
  searchTags: SearchTagOption[],
  builtinLabel: (id: string) => string
): LibraryTag[] {
  const countOf = (id: string) => searchTags.find(tag => tag.id === id)?.count ?? 0
  return [
    ...LIBRARY_BUILTIN_TAG_IDS.map(id => ({
      tagId: id,
      name: builtinLabel(id),
      entryCount: countOf(id),
      createdAtMs: 0,
      builtin: true,
    })),
    ...local.map(tag => ({
      tagId: tag.tagId,
      name: tag.name ?? null,
      entryCount: tag.entryCount,
      createdAtMs: tag.createdAtMs,
      builtin: false,
    })),
  ]
}

/** What sorting and similarity read of a tag. */
type SortableTag = Pick<HistoryTagDto, 'tagId' | 'entryCount' | 'createdAtMs'> & {
  name?: string | null
}

/** The Library's tag orders (HManage.dc.html sort control). */
export const TAG_SORTS = ['mostUsed', 'name', 'newest'] as const
export type TagSort = (typeof TAG_SORTS)[number]

/** `tags` in the chosen order; unreadable names sort last by name. */
export function sortTags<T extends SortableTag>(tags: T[], sort: TagSort): T[] {
  const byName = (a: T, b: T) =>
    a.name == null ? (b.name == null ? 0 : 1) : b.name == null ? -1 : a.name.localeCompare(b.name)
  const compare: Record<TagSort, (a: T, b: T) => number> = {
    mostUsed: (a, b) => b.entryCount - a.entryCount || byName(a, b),
    name: byName,
    newest: (a, b) => b.createdAtMs - a.createdAtMs || byName(a, b),
  }
  return [...tags].sort(compare[sort])
}

/** A name reduced to what tells tags apart: case, spacing and punctuation go. */
function normalize(name: string): string {
  return name.toLocaleLowerCase().replace(/[\s\-_.]+/g, '')
}

/** Levenshtein distance, stopping early once it exceeds `limit`. */
function withinEdits(a: string, b: string, limit: number): boolean {
  if (Math.abs(a.length - b.length) > limit) return false
  let previous = Array.from({ length: b.length + 1 }, (_, j) => j)
  for (let i = 1; i <= a.length; i++) {
    const row = [i]
    for (let j = 1; j <= b.length; j++) {
      row[j] = Math.min(
        previous[j] + 1,
        row[j - 1] + 1,
        previous[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1)
      )
    }
    if (Math.min(...row) > limit) return false
    previous = row
  }
  return previous[b.length] <= limit
}

/** Whether two tag names likely mean the same thing: equal once normalized,
 * one a prefix of the other (`release` / `releases`), or a typo apart. */
export function similarNames(a: string, b: string): boolean {
  const x = normalize(a)
  const y = normalize(b)
  if (x === '' || y === '') return false
  if (x === y) return true
  const [short, long] = x.length <= y.length ? [x, y] : [y, x]
  // A short stem only counts when the longer name barely extends it
  // (`release` / `releases`, not `git` / `github`).
  if (short.length >= 4 && long.startsWith(short) && long.length - short.length <= 2) return true
  return short.length >= 4 && withinEdits(x, y, short.length >= 7 ? 2 : 1)
}

/**
 * The tag `tag` most likely duplicates (HManage.dc.html "similar to #design"),
 * the most used one when several are: what its Merge suggests first. `null`
 * when none is close, or the name cannot be read.
 */
export function similarTagOf<T extends SortableTag>(tag: T, tags: T[]): T | null {
  const name = tag.name
  if (!name) return null
  let best: T | null = null
  for (const other of tags) {
    if (other.tagId === tag.tagId || !other.name || !similarNames(name, other.name)) continue
    if (!best || other.entryCount > best.entryCount) best = other
  }
  return best
}
