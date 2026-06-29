import {
  AlertTriangle,
  ArrowDownToLine,
  ArrowUpFromLine,
  FileText,
  LoaderCircle,
  Star,
} from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import type { EntrySourceView } from '@/api/tauri-command/clipboard_delivery'
import type { ClipboardCodeItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { cn } from '@/lib/utils'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'
import {
  describeSource,
  detectCodeLanguage,
  getContentSizeLabel,
  TAG_STYLE,
  TYPE_COLOR,
  TYPE_ICONS,
} from './history-card-utils'

interface HistoryCardHeaderProps {
  item: DisplayClipboardItem
  relativeTime: string
  deliverySource?: EntrySourceView
  transfer?: TransferProgressInfo
  state: {
    isFileType: boolean
    isFavorited: boolean
    isUnavailable: boolean
    isTransferring: boolean
    isPending: boolean
  }
  percent: number
}

function HistoryCardHeader({
  item,
  relativeTime,
  deliverySource,
  transfer,
  state,
  percent,
}: HistoryCardHeaderProps) {
  const { t } = useTranslation()
  const { isFileType, isFavorited, isUnavailable, isTransferring, isPending } = state
  const headerType = item.contentTags?.length ? 'text' : item.type
  const color = TYPE_COLOR[headerType] ?? TYPE_COLOR.unknown
  const TypeIcon = TYPE_ICONS[headerType] ?? FileText
  const sizeLabel = useMemo(() => getContentSizeLabel(item, t), [item, t])
  const codeLanguage = useMemo(
    () =>
      item.type === 'code'
        ? detectCodeLanguage(
            (item.content as ClipboardCodeItem | null)?.code ?? item.textPreview ?? ''
          )
        : null,
    [item]
  )
  const DirectionIcon = transfer?.direction === 'Sending' ? ArrowUpFromLine : ArrowDownToLine
  const speedLabel = transfer?.bytesPerSecond
    ? formatFileSize(transfer.bytesPerSecond) + '/s'
    : null
  const source = deliverySource ? describeSource(deliverySource, t) : null

  return (
    <div className="pointer-events-none relative z-10 mb-1.5 flex items-center gap-1.5">
      <TypeIcon className={cn('size-3 shrink-0', isPending && 'opacity-50')} style={{ color }} />
      <span
        className={cn('text-[10.5px] font-medium', isPending && 'opacity-50')}
        style={{ color }}
      >
        {item.contentTags?.length
          ? t('history.type.text', 'text')
          : (codeLanguage ?? t(`history.type.${item.type}`, item.type))}
      </span>

      {item.contentTags?.map(tag => (
        <span
          key={tag}
          className="rounded border px-1 py-0 text-[9px] font-medium leading-[1.25]"
          style={{
            backgroundColor: TAG_STYLE[tag].background,
            borderColor: TAG_STYLE[tag].border,
            color: TAG_STYLE[tag].text,
          }}
        >
          {t(`history.type.${tag}`, tag)}
        </span>
      ))}

      {sizeLabel && !isTransferring && (
        <>
          <span className="text-[9px] text-muted-foreground/25">·</span>
          <span className="truncate text-[10px] tabular-nums text-muted-foreground/45">
            {sizeLabel}
          </span>
        </>
      )}

      <div className="ml-auto flex shrink-0 items-center gap-1.5">
        {isUnavailable && (
          <AlertTriangle
            className="size-2.5 text-amber-500/70"
            aria-label={t('clipboard.errors.unavailableBadge')}
          />
        )}
        {isFavorited && <Star className="size-2.5 fill-amber-400 text-amber-400" />}
        {isFileType && isTransferring ? (
          <>
            <DirectionIcon className="size-2.5 text-primary/70" />
            <span className="text-[10px] font-medium tabular-nums text-primary/80">{percent}%</span>
            {speedLabel && (
              <>
                <span className="text-[9px] text-primary/30">·</span>
                <span className="text-[10px] tabular-nums text-primary/70">{speedLabel}</span>
              </>
            )}
          </>
        ) : isFileType && isPending ? (
          <>
            <LoaderCircle className="size-2.5 animate-spin text-muted-foreground/40" />
            <span className="text-[10px] text-muted-foreground/40">
              {t('clipboard.transfer.pending')}
            </span>
          </>
        ) : (
          <span className="flex items-center gap-1 text-[10px] text-muted-foreground/45">
            {source?.Icon && <source.Icon className={cn('size-2.5', source.color)} />}
            {source?.label && (
              <>
                <span className="max-w-[7rem] truncate">{source.label}</span>
                <span className="text-muted-foreground/25">·</span>
              </>
            )}
            <span className="tabular-nums">{relativeTime}</span>
          </span>
        )}
      </div>
    </div>
  )
}

export default HistoryCardHeader
