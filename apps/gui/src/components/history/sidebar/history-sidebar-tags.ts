import type { SearchTagOption } from '@/lib/search-tags'

/** A Tags row of the Library sidebar: a search tag, or the `file` content
 * type, which is not a tag but is filed beside them. */
export type SidebarTagRow =
  | { kind: 'tag'; id: string; count: number }
  | { kind: 'file'; id: 'file'; count: null }

// Builtin rows always show, in this order, even while empty; `favorited` is
// left out because the Pinned row already is that tag.
const BUILTIN_ROWS = ['link', 'code', 'image', 'file', 'directory'] as const

/** The Tags rows: the builtin rows, then custom tags that hold at least one
 * item, most used first. The `file` row carries no count: `/search/tags` does
 * not count content types. */
export function sidebarTagRows(tags: SearchTagOption[]): SidebarTagRow[] {
  const countOf = (id: string) => tags.find(tag => tag.id === id)?.count ?? 0
  const builtins = BUILTIN_ROWS.map((id): SidebarTagRow =>
    id === 'file' ? { kind: 'file', id, count: null } : { kind: 'tag', id, count: countOf(id) }
  )
  const custom = tags
    .filter(tag => !tag.isBuiltin && tag.count > 0)
    .sort((a, b) => b.count - a.count || a.id.localeCompare(b.id))
    .map((tag): SidebarTagRow => ({ kind: 'tag', id: tag.id, count: tag.count }))
  return [...builtins, ...custom]
}

const BUILTIN_DOT_CLASSES: Record<string, string> = {
  link: 'bg-info',
  code: 'bg-violet-500',
  image: 'bg-success',
  file: 'bg-warning',
  directory: 'bg-muted-foreground',
}

const CUSTOM_DOT_CLASSES = [
  'bg-info',
  'bg-success',
  'bg-warning',
  'bg-violet-500',
  'bg-muted-foreground',
] as const

/** A stable colour for a row's dot: fixed for builtin rows, derived from the id
 * for custom tags (tags carry no colour). */
export function tagDotClass(id: string): string {
  const builtin = BUILTIN_DOT_CLASSES[id]
  if (builtin) return builtin
  let hash = 0
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) | 0
  return CUSTOM_DOT_CLASSES[Math.abs(hash) % CUSTOM_DOT_CLASSES.length]
}
