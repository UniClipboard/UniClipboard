import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

/** The visual kind HList.dc.html / HDetail.dc.html badge an entry with. */
export type HistoryKind = 'text' | 'code' | 'link' | 'image' | 'file'

export function historyKind(item: DisplayClipboardItem): HistoryKind {
  if (item.type === 'image') return 'image'
  if (item.type === 'file') return 'file'
  if (item.contentTags?.includes('code')) return 'code'
  if (item.contentTags?.includes('link')) return 'link'
  return 'text'
}

/** Badge glyphs are visual shorthand (aria-hidden); the row's kind is spoken
 * through the detail pill's translated `history.type.*` label. */
export const KIND_GLYPH: Record<Exclude<HistoryKind, 'image'>, string> = {
  text: 'TXT',
  code: '</>',
  link: 'URL',
  file: 'FILE',
}

/** Shared tint for the list badge and the detail type pill. */
export const KIND_TINT: Record<HistoryKind, string> = {
  text: 'bg-muted text-foreground/75',
  code: 'bg-zinc-900 text-green-400 dark:bg-black/60',
  link: 'bg-blue-500/10 text-blue-800 dark:text-blue-300',
  image: 'bg-orange-500/12 text-orange-800 dark:text-orange-300',
  file: 'bg-orange-500/12 text-orange-800 dark:text-orange-300',
}

/** Tags a row shows as chips (HList.dc.html), most telling first. */
export type HistoryRowTag = 'link' | 'code' | 'directory'

/** A chip on a row: one of this device's local tags (`name` is `null` when it
 * cannot be read), or a builtin content tag. */
export type HistoryRowChip =
  | { kind: 'local'; id: string; name: string | null }
  | { kind: 'builtin'; tag: HistoryRowTag }

/** Most chips a row shows. */
const ROW_CHIP_LIMIT = 2

/** The row's tag chips (HList.dc.html), at most two: its local tags first,
 * then its builtin tags minus the one its kind badge already shows (no `#link`
 * beside a URL badge). Local tags show only once `tagNames` knows them. */
export function rowChips(
  item: DisplayClipboardItem,
  kind: HistoryKind,
  tagNames?: ReadonlyMap<string, string | null>
): HistoryRowChip[] {
  const chips: HistoryRowChip[] = []
  for (const id of item.userTagIds ?? []) {
    if (tagNames?.has(id)) chips.push({ kind: 'local', id, name: tagNames.get(id) ?? null })
  }
  for (const tag of item.contentTags ?? []) {
    if ((tag === 'link' || tag === 'code') && tag !== kind) chips.push({ kind: 'builtin', tag })
  }
  if (item.isDirectory) chips.push({ kind: 'builtin', tag: 'directory' })
  return chips.slice(0, ROW_CHIP_LIMIT)
}
