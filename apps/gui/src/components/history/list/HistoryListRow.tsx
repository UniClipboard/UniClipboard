import { Pin } from 'lucide-react'
import React, { useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import InlineTextSummary from '@/components/clipboard/InlineTextSummary'
import {
  fileNameFromPreview,
  imageTitle,
} from '@/components/history/history-card/history-card-utils'
import HistoryCardTransferProgress from '@/components/history/history-card/HistoryCardTransferProgress'
import { useResourceImageUrl } from '@/components/history/history-card/useResourceImageUrl'
import { useTagTints } from '@/components/history/tags/tag-colors-context'
import { Checkbox } from '@/components/ui/checkbox'
import type {
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { linkItemFromTextContent } from '@/lib/clipboard-utils'
import { tagLabel } from '@/lib/search-tags'
import { cn } from '@/lib/utils'
import { useAppSelector } from '@/store/hooks'
import {
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
} from '@/store/slices/fileTransferSlice'
import { formatClockTime } from './history-list-format'
import { historyKind, KIND_GLYPH, KIND_TINT, rowChips, type HistoryKind } from './history-list-kind'

interface HistoryListRowProps {
  item: DisplayClipboardItem
  /** The origin device's name; the meta line leads with it when known. */
  deviceName?: string
  /** Bulk selection: this row is checked / any row is checked. The boxes
   * show only while something is checked; ⌘- or Ctrl-click starts it. */
  checked: boolean
  anyChecked: boolean
  onToggleChecked: (id: string) => void
  copySuccess: boolean
  isDeleting: boolean
  onClick: (id: string) => void
  onHoverChange: (id: string, hovered: boolean) => void
  /** Local tag id -> name; absent while local tags are unavailable. */
  tagNames?: ReadonlyMap<string, string | null>
}

/** One-line title for a row: the text itself, the link, the file name, or the
 * image label with its dimensions. */
function rowTitle(item: DisplayClipboardItem, kind: HistoryKind, imageLabel: string): string {
  if (kind === 'image') return imageTitle(imageLabel, null, item.content as ClipboardImageItem)
  if (kind === 'file') {
    const file = item.content as ClipboardFileItem | null
    if (file?.file_names.length) {
      const extra = file.file_names.length - 1
      return extra > 0 ? `${file.file_names[0]} +${extra}` : file.file_names[0]
    }
    return item.textPreview ? fileNameFromPreview(item.textPreview) : ''
  }
  const text = item.content as ClipboardTextItem | null
  if (kind === 'link' && text) {
    const link = linkItemFromTextContent(text)
    if (link?.urls[0]) return link.urls[0]
  }
  return text?.display_text ?? item.textPreview ?? ''
}

function KindBadge({ item, kind }: { item: DisplayClipboardItem; kind: HistoryKind }) {
  return (
    <span className="flex w-12 shrink-0 justify-center" aria-hidden="true">
      {kind === 'image' ? (
        <ImageThumb entryId={item.id} />
      ) : (
        <span
          className={cn(
            'flex size-7.5 items-center justify-center rounded-lg font-mono text-ui-caption font-semibold',
            KIND_TINT[kind]
          )}
        >
          {KIND_GLYPH[kind]}
        </span>
      )}
    </span>
  )
}

function ImageThumb({ entryId }: { entryId: string }) {
  const url = useResourceImageUrl(entryId)
  return url ? (
    <img
      src={url}
      alt=""
      className="h-9 w-12 rounded-md border border-border object-cover"
      draggable={false}
    />
  ) : (
    <span className={cn('h-9 w-12 rounded-md border border-border', KIND_TINT.image)} />
  )
}

/**
 * macOS history row (HList.dc.html): kind badge, one-line title, a meta line of
 * origin device and copy time, and tag chips. The design's source app is not
 * recorded, so the meta line leaves it out. Unlike HistoryCard it has no
 * hover action bar: actions live in the context menu, the detail column and
 * the bulk bar.
 */
function HistoryListRow({
  item,
  deviceName,
  checked,
  anyChecked,
  onToggleChecked,
  copySuccess,
  isDeleting,
  onClick,
  onHoverChange,
  tagNames,
}: HistoryListRowProps) {
  const { t, i18n } = useTranslation()
  const tintOf = useTagTints()
  const kind = historyKind(item)
  const isFileType = item.type === 'file'
  const transfer = useAppSelector(state =>
    isFileType ? selectTransferByEntryId(state, item.id) : undefined
  )
  const entryStatus = useAppSelector(state =>
    isFileType ? selectEntryTransferStatus(state, item.id) : undefined
  )
  const status = isFileType ? resolveEntryTransferStatus(entryStatus, transfer) : undefined
  const percent =
    transfer && transfer.totalBytes && transfer.totalBytes > 0
      ? Math.round((transfer.bytesTransferred / transfer.totalBytes) * 100)
      : 0
  // Same rule as HistoryCard: a directory send's byte percentage is meaningless.
  const hideByteProgress = (item.isDirectory ?? false) && transfer?.direction === 'sending'

  // The hovered row still drives the page's hover shortcuts (c / d).
  useEffect(() => () => onHoverChange(item.id, false), [item.id, onHoverChange])
  const handleMouseEnter = useCallback(() => onHoverChange(item.id, true), [item.id, onHoverChange])
  const handleMouseLeave = useCallback(
    () => onHoverChange(item.id, false),
    [item.id, onHoverChange]
  )
  // ⌘- or Ctrl-click toggles the row's check instead of opening it. On macOS a
  // Ctrl-click is also the secondary click: it arrives as `contextmenu`
  // (button 0), which is claimed here before the row's context menu opens.
  // WebKit may still follow it with a `click`; the timestamp drops that one.
  const ctrlToggledAt = useRef(0)
  const handleContextMenu = useCallback(
    (event: React.MouseEvent) => {
      if (!event.ctrlKey || event.button !== 0) return
      event.preventDefault()
      ctrlToggledAt.current = event.timeStamp
      onToggleChecked(item.id)
    },
    [item.id, onToggleChecked]
  )
  const handleClick = useCallback(
    (event: React.MouseEvent) => {
      if (!event.metaKey && !event.ctrlKey) {
        onClick(item.id)
        return
      }
      if (event.ctrlKey && event.timeStamp - ctrlToggledAt.current < 500) return
      onToggleChecked(item.id)
    },
    [item.id, onClick, onToggleChecked]
  )

  const title = rowTitle(item, kind, t('history.type.image', 'image'))
  const chips = rowChips(item, kind, tagNames)

  return (
    <div
      data-testid="history-card"
      data-entry-id={item.id}
      data-favorited={item.isFavorited ?? false}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onContextMenu={handleContextMenu}
      className={cn(
        // With the wrapper's 0.5rem inset, content still starts 1.125rem in.
        'relative flex h-14 cursor-pointer items-center gap-3 pl-2.5 pr-2 transition-colors',
        isDeleting
          ? 'bg-destructive/10 opacity-60'
          : copySuccess
            ? 'bg-emerald-500/5'
            : checked
              ? 'bg-primary/5'
              : 'hover:bg-muted/40',
        item.isUnavailable && 'opacity-55',
        status === 'pending' && 'opacity-60'
      )}
    >
      <button
        type="button"
        aria-label={t('clipboard.item.actions.open', 'Open clipboard item')}
        onClick={handleClick}
        className="absolute inset-0 z-[1] cursor-pointer appearance-none border-0 bg-transparent p-0 outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-primary/40"
      />
      <HistoryCardTransferProgress
        isFileType={isFileType}
        isTransferring={status === 'transferring'}
        transfer={transfer}
        percent={percent}
        hideByteProgress={hideByteProgress}
      />
      {anyChecked && (
        <Checkbox
          checked={checked}
          onCheckedChange={() => onToggleChecked(item.id)}
          aria-label={t('history.list.selectItem', { title })}
          className="relative z-10 size-4.5 rounded-[5px] border-[1.5px] border-muted-foreground/45 bg-background data-checked:border-foreground data-checked:bg-foreground data-checked:text-background dark:data-checked:bg-foreground"
        />
      )}
      <KindBadge item={item} kind={kind} />
      <span className="pointer-events-none relative z-10 flex min-w-0 flex-1 flex-col gap-0.5">
        <span
          className={cn(
            'truncate text-ui-body font-medium text-foreground',
            kind === 'code' && 'font-mono'
          )}
        >
          <InlineTextSummary text={title} />
        </span>
        <span className="truncate text-ui-caption text-muted-foreground">
          {/* A list column under 400px keeps only the time. */}
          {deviceName && <span className="@max-[25rem]:hidden">{deviceName} · </span>}
          <span className="font-mono tabular-nums">
            {formatClockTime(item.activeTime, i18n.language)}
          </span>
        </span>
      </span>
      {chips.length > 0 && (
        <span className="relative z-10 flex min-w-0 shrink-0 gap-1 @max-[25rem]:hidden">
          {chips.map(chip => {
            const tint = tintOf(chip.kind === 'local' ? chip.id : chip.tag)
            return (
              <span
                key={chip.kind === 'local' ? chip.id : chip.tag}
                style={tint.style}
                className={cn(
                  'inline-flex h-5 max-w-28 items-center truncate rounded-full px-1.75 text-ui-caption font-semibold',
                  tint.chip
                )}
              >
                #
                {chip.kind === 'local'
                  ? tagLabel({ id: chip.id, name: chip.name, isBuiltin: false }, t)
                  : t(`history.type.${chip.tag}`)}
              </span>
            )
          })}
        </span>
      )}
      {item.isFavorited && (
        <span role="img" aria-label={t('history.sidebar.pinned')} className="relative z-10">
          <Pin className="size-3.5 shrink-0 text-orange-500" aria-hidden="true" />
        </span>
      )}
    </div>
  )
}

export default React.memo(HistoryListRow)
