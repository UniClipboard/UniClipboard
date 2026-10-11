import type { HistoryTagColorDto } from '@/api/daemon/history-tags'
import { dayKey, formatDayHeading } from '@/components/history/list/history-list-format'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { isCustomColor, isPresetColor } from '@/lib/tag-colors'
import type { ListGroup, TagColor } from '@/ui-kit'

type Translate = (key: string, opts?: Record<string, unknown>) => string

/** Day groups of the loaded rows: one group per run of rows copied on the same calendar day. */
export function dayGroups(
  items: DisplayClipboardItem[],
  now: number,
  locale: string,
  t: Translate
): ListGroup[] {
  const groups: ListGroup[] = []
  let current: number | null = null
  for (const item of items) {
    const day = dayKey(item.activeTime)
    if (day === current) {
      groups[groups.length - 1].count += 1
      continue
    }
    current = day
    const { title, detail } = formatDayHeading(item.activeTime, now, locale, t)
    groups.push({ id: String(day), title, detail, count: 1 })
  }
  for (const group of groups) group.trailing = t('history.subtitle', { count: group.count })
  return groups
}

/** A daemon tag colour as a UI kit colour; anything unknown is drawn neutral. */
export function kitTagColor(color: HistoryTagColorDto | undefined): TagColor | undefined {
  if (!color) return undefined
  if (isPresetColor(color)) return color
  if (isCustomColor(color)) return color as `#${string}`
  return undefined
}
