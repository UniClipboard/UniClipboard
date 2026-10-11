import { useTranslation } from 'react-i18next'
import { deviceLabel } from '@/components/clipboard/entry-delivery-labels'
import { formatClockTime } from '@/components/history/list/history-list-format'
import { historyKind } from '@/components/history/list/history-list-kind'
import { rowTitle } from '@/components/history/list/history-row-title'
import { useNow } from '@/hooks/useRelativeTime'
import type { ClipboardTextItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { GroupedGrid, Icon, type GridItem, type MenuEntry } from '@/ui-kit'
import { dayGroups, imageSize } from './history-kit-model'
import { HistoryKitThumbnail } from './HistoryKitThumbnail'

interface HistoryKitGridProps {
  items: DisplayClipboardItem[]
  sourceDeviceNames: Record<string, string>
  size: number
  selection: string[]
  onSelectionChange: (ids: string[]) => void
  onOpen: (id: string) => void
  onDelete: (ids: string[]) => void
  hasMore: boolean
  onLoadMore: () => void
  contextMenu: {
    items: (ids: string[]) => MenuEntry[]
    onAction: (action: string, ids: string[]) => void
  }
}

/**
 * The grid layout (design ME2): cards by type in day groups. An image card shows the picture, code
 * a dark snippet, a link its site and address, text the text itself. The design's colour cards are
 * not drawn: the Engine has no colour entry type.
 */
export function HistoryKitGrid({
  items,
  sourceDeviceNames,
  size,
  selection,
  onSelectionChange,
  onOpen,
  onDelete,
  hasMore,
  onLoadMore,
  contextMenu,
}: HistoryKitGridProps) {
  const { t, i18n } = useTranslation()
  const now = useNow()
  const cards: GridItem[] = items.map(item => {
    const kind = historyKind(item)
    const device = item.sourceDeviceId
      ? deviceLabel(sourceDeviceNames[item.sourceDeviceId], item.sourceDeviceId)
      : null
    const title = rowTitle(item, kind, t('history.type.image', 'image'))
    const text =
      kind === 'image' || kind === 'file'
        ? title
        : ((item.content as ClipboardTextItem | null)?.display_text ?? item.textPreview ?? title)
    return {
      id: item.id,
      kind,
      title: kind === 'code' || kind === 'text' ? t(`history.type.${kind}`) : title,
      subtitle: [device, imageSize(item) ?? formatClockTime(item.activeTime, i18n.language)]
        .filter(Boolean)
        .join(' · '),
      text: kind === 'link' ? title : text,
      media: kind === 'image' ? <HistoryKitThumbnail entryId={item.id} /> : undefined,
      badges:
        item.isFavorited === true ? (
          <Icon name="pin" tone="highlight" label={t('history.sidebar.pinned')} />
        ) : undefined,
    }
  })
  return (
    <GroupedGrid
      label={t('history.kit.listLabel')}
      groups={dayGroups(items, now, i18n.language, t)}
      items={cards}
      size={size}
      selection={selection}
      onSelectionChange={onSelectionChange}
      onActivate={onOpen}
      onDelete={onDelete}
      onEndReached={hasMore ? onLoadMore : undefined}
      contextMenu={contextMenu}
    />
  )
}
