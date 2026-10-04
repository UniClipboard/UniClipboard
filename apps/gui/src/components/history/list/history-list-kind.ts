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

/** Chip tints; hues match the sidebar's Tags dots (`tagDotClass`). */
export const ROW_TAG_TINT: Record<HistoryRowTag, string> = {
  link: 'bg-blue-500/10 text-blue-800 dark:text-blue-300',
  code: 'bg-violet-500/12 text-violet-800 dark:text-violet-300',
  directory: 'bg-muted text-foreground/75',
}

/** The row's tag chips, at most two: its tags minus the one its kind badge
 * already shows (no `#link` beside a URL badge). */
export function rowTags(item: DisplayClipboardItem, kind: HistoryKind): HistoryRowTag[] {
  const tags: HistoryRowTag[] = []
  for (const tag of item.contentTags ?? []) {
    if ((tag === 'link' || tag === 'code') && tag !== kind) tags.push(tag)
  }
  if (item.isDirectory) tags.push('directory')
  return tags.slice(0, 2)
}
