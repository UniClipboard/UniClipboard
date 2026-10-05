import { isBuiltinTagId, type SearchTagOption } from '@/lib/search-tags'

/** A Tags row of the Library sidebar: a builtin tag or a local one. */
export interface SidebarTagRow {
  id: string
  count: number
  isBuiltin: boolean
  name?: string | null
}

/** Most tags the sidebar lists; the rest are one "All tags" link away. */
export const SIDEBAR_TAG_LIMIT = 6

/** The Tags rows (HSidebar.dc.html): the first `SIDEBAR_TAG_LIMIT` tags of the
 * daemon's sidebar layout, in its order, with their counts and names from the
 * search tags. Ids the search tags do not know yet count 0. */
export function sidebarTagRows(
  sidebarTagIds: readonly string[],
  tags: SearchTagOption[]
): SidebarTagRow[] {
  const byId = new Map(tags.map(tag => [tag.id, tag]))
  return sidebarTagIds.slice(0, SIDEBAR_TAG_LIMIT).map(id => {
    const tag = byId.get(id)
    return {
      id,
      count: tag?.count ?? 0,
      isBuiltin: tag?.isBuiltin ?? isBuiltinTagId(id),
      name: tag?.name,
    }
  })
}
