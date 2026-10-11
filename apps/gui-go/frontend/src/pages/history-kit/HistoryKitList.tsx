import { useTranslation } from 'react-i18next'
import { deviceLabel } from '@/components/clipboard/entry-delivery-labels'
import { formatClockTime } from '@/components/history/list/history-list-format'
import { historyKind, rowChips } from '@/components/history/list/history-list-kind'
import { rowTitle } from '@/components/history/list/history-row-title'
import { useNow } from '@/hooks/useRelativeTime'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { GroupedList, Text, type ItemListItem, type MenuEntry } from '@/ui-kit'
import { dayGroups, imageSize } from './history-kit-model'
import { HistoryKitRowLeading } from './HistoryKitRowLeading'
import { HistoryKitRowTrailing } from './HistoryKitRowTrailing'

interface HistoryKitListProps {
  items: DisplayClipboardItem[]
  sourceDeviceNames: Record<string, string>
  tagNames?: ReadonlyMap<string, string | null>
  selection: string[]
  onSelectionChange: (ids: string[]) => void
  onCopy: (id: string) => void
  onDelete: (ids: string[]) => void
  hasMore: boolean
  loading: boolean
  onLoadMore: () => void
  contextMenu: {
    items: (ids: string[]) => MenuEntry[]
    onAction: (action: string, ids: string[]) => void
  }
  /** Search terms to mark in the titles. */
  highlight?: string[]
}

/**
 * The history list (design MA2): loaded rows grouped by the day they were copied. Each row shows
 * its type mark or thumbnail, a one-line title, the origin device and time, its pin mark and up to
 * two tags. The source app in the design's meta line is not recorded by the Engine, so it is left
 * out.
 */
export function HistoryKitList({
  items,
  sourceDeviceNames,
  tagNames,
  selection,
  onSelectionChange,
  onCopy,
  onDelete,
  hasMore,
  loading,
  onLoadMore,
  contextMenu,
  highlight,
}: HistoryKitListProps) {
  const { t, i18n } = useTranslation()
  const now = useNow()
  const groups = dayGroups(items, now, i18n.language, t)
  const rows: ItemListItem[] = items.map(item => {
    const kind = historyKind(item)
    const device = item.sourceDeviceId
      ? deviceLabel(sourceDeviceNames[item.sourceDeviceId], item.sourceDeviceId)
      : null
    return {
      id: item.id,
      title: rowTitle(item, kind, t('history.type.image', 'image')),
      // An image row names its size instead of the time (design ME1).
      subtitle: [device, imageSize(item) ?? formatClockTime(item.activeTime, i18n.language)]
        .filter(Boolean)
        .join(' · '),
      monospace: kind === 'code',
      highlight,
      leading: <HistoryKitRowLeading entryId={item.id} kind={kind} />,
      trailing: (
        <HistoryKitRowTrailing
          pinned={item.isFavorited === true}
          chips={rowChips(item, kind, tagNames)}
        />
      ),
    }
  })
  return (
    <GroupedList
      label={t('history.kit.listLabel')}
      groups={groups}
      items={rows}
      selection={selection}
      onSelectionChange={onSelectionChange}
      onActivate={onCopy}
      onDelete={onDelete}
      onEndReached={hasMore ? onLoadMore : undefined}
      contextMenu={contextMenu}
      footer={
        loading || hasMore ? (
          <Text role="caption" tone="secondary">
            {t('clipboard.item.loading')}
          </Text>
        ) : selection.length > 1 ? (
          <Text role="caption" tone="tertiary">
            {t('history.kit.multiSelectHint', {
              mod: formatShortcutHint('mod'),
              shift: formatShortcutHint('shift'),
              all: formatShortcutHint('mod+a'),
            })}
          </Text>
        ) : null
      }
    />
  )
}
