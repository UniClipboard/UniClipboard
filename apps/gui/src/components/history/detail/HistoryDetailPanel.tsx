import { Check, Download, ExternalLink, Pin, Send, Trash2 } from 'lucide-react'
import React from 'react'
import { Trans, useTranslation } from 'react-i18next'
import { PreviewContent } from '@/components/clipboard/ClipboardPreview'
import ClipboardSendMenu from '@/components/clipboard/ClipboardSendMenu'
import { isLargeTextPreview } from '@/components/clipboard/preview-renderers/textPreviewUtils'
import TransferProgressBar from '@/components/clipboard/TransferProgressBar'
import {
  describeSource,
  getContentSizeLabel,
} from '@/components/history/history-card/history-card-utils'
import { formatCopiedAt } from '@/components/history/list/history-list-format'
import { historyKind, KIND_TINT } from '@/components/history/list/history-list-kind'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import { useCancelEntryTransfer } from '@/hooks/useCancelEntryTransfer'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type {
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { imageFormatLabel } from '@/lib/image-handoff'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils'
import { DETAIL_SECTION_LABEL } from './detail-styles'
import HistoryDetailDelivery from './HistoryDetailDelivery'
import HistoryDetailTags, { type DetailTagsProps } from './HistoryDetailTags'
import ImageQuickLook from './ImageQuickLook'
import { useHistoryImageActions } from './useHistoryImageActions'

interface HistoryDetailPanelProps {
  item: DisplayClipboardItem | null
  /** Local tags for the entry; absent or `null` hides the TAGS block. */
  tagging?: DetailTagsProps | null
  copySuccess: boolean
  onCopy: () => void
  onToggleFavorite: () => void
  onDelete: () => void
}

const iconButton =
  'flex size-8.5 shrink-0 items-center justify-center rounded-[0.5625rem] border transition-colors'

const pillButton =
  'inline-flex h-10.5 items-center gap-2 rounded-full px-5 text-ui-body font-medium transition-colors disabled:opacity-50'

// At most three cards: Copied, Size, and one of Dimensions, Files or Stored.
const GRID_COLS: Record<number, string> = {
  1: 'grid-cols-1',
  2: 'grid-cols-2',
  3: 'grid-cols-3',
}

interface MetaCardProps {
  id: string
  label: string
  value: string
  mono?: boolean
}

function MetaCard({ label, value, mono }: Omit<MetaCardProps, 'id'>) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-[0.625rem] bg-muted px-3 py-2.5">
      <span className={DETAIL_SECTION_LABEL}>{label}</span>
      <span className={cn('truncate text-ui-body font-medium', mono && 'font-mono tabular-nums')}>
        {value}
      </span>
    </div>
  )
}

/**
 * macOS detail column (HDetail.dc.html, `item` variant): header with the kind
 * pill, source and pin/delete; the content; copy facts; per-device delivery;
 * and a footer with Copy and Send.
 */
const HistoryDetailPanel: React.FC<HistoryDetailPanelProps> = ({
  item,
  tagging,
  copySuccess,
  onCopy,
  onToggleFavorite,
  onDelete,
}) => {
  const { t, i18n } = useTranslation()
  const state = useClipboardPreviewState(item)
  const { delivery } = useEntryDelivery(item?.id ?? null)
  const { cancelling, cancel } = useCancelEntryTransfer(item?.id, state.transfer)
  const imageDescriptor =
    state.preview?.contentType === 'image' ? (state.preview.imageBlobPath ?? null) : null
  const imageActions = useHistoryImageActions(
    item?.id,
    imageDescriptor,
    item?.type === 'image' && !item.isUnavailable
  )
  // Cached by path, so this is the object URL the stage already shows.
  const quickLookSrc = useBlobImageObjectUrl(imageDescriptor, imageActions.quickLook)

  if (!item) {
    return (
      <section
        aria-label={t('history.detail.aria')}
        className="flex h-full flex-col items-center justify-center gap-2 bg-muted/20 text-center"
        data-testid="clipboard-detail"
      >
        <span className="text-ui-section">{t('history.detail.emptyTitle')}</span>
        <span className="text-ui-body text-muted-foreground">{t('history.detail.emptyHint')}</span>
      </section>
    )
  }

  const kind = historyKind(item)
  const isFavorited = item.isFavorited === true
  const source = delivery ? describeSource(delivery.source, t) : null

  const isLargeText =
    (item.type === 'text' || item.type === 'richtext') &&
    item.content !== null &&
    isLargeTextPreview(item.content as ClipboardTextItem, state.preview, state.loading)
  // Code and large text own their scrolling; everything else scrolls in the box.
  const isImage = item.type === 'image'
  const fillsBox = isLargeText || kind === 'code' || isImage
  // Text, code and links start at the design's 170px box; the box grows with
  // the window height, and the facts below keep their natural size.
  const compactBox = kind === 'text' || kind === 'code' || kind === 'link'
  const content = (
    <PreviewContent
      item={item}
      loading={state.loading}
      preview={state.preview}
      effectiveStatus={state.effectiveStatus}
      entryStatus={state.entryStatus}
      transfer={state.transfer}
      setImageDimensions={state.setImageDimensions}
      codeVariant="block"
      imageFit
      imageActualSize={imageActions.mode === 'actual'}
      onImageType={imageActions.onImageType}
    />
  )

  const copied: MetaCardProps = {
    id: 'copied',
    label: t('history.detail.copied'),
    value: formatCopiedAt(item.activeTime, i18n.language),
    mono: true,
  }
  const image = item.type === 'image' ? (item.content as ClipboardImageItem | null) : null
  const sizeLabel =
    image && image.size > 0
      ? formatFileSize(image.size)
      : isImage && state.preview && state.preview.sizeBytes > 0
        ? formatFileSize(state.preview.sizeBytes)
        : getContentSizeLabel(item, t)
  const imageFormat = imageActions.formatLabel ?? imageFormatLabel(state.preview?.mimeType)
  const size: MetaCardProps | null = sizeLabel
    ? {
        id: 'size',
        label: t('history.detail.size'),
        value: isImage && imageFormat ? `${imageFormat} · ${sizeLabel}` : sizeLabel,
      }
    : null
  const dims =
    state.imageDimensions ??
    (image && image.width > 0 ? { width: image.width, height: image.height } : null)
  const files = item.type === 'file' ? (item.content as ClipboardFileItem | null) : null
  // A third card only appears for multi-file entries (the file count).
  const third: MetaCardProps | null =
    !item.isUnavailable && files && files.file_names.length > 1
      ? {
          id: 'files',
          label: t('history.detail.fileCount'),
          value: String(files.file_names.length),
        }
      : null
  // Design order: images lead with Dimensions and end with Copied; everything
  // else reads Copied, Size, then Files (multi-file).
  const cards: MetaCardProps[] = isImage
    ? [
        ...(dims
          ? [
              {
                id: 'dims',
                label: t('clipboard.preview.dimensions'),
                value: `${dims.width} × ${dims.height}`,
              },
            ]
          : []),
        ...(size ? [size] : []),
        copied,
      ]
    : [copied, ...(size ? [size] : []), ...(third ? [third] : [])]

  return (
    <section
      aria-label={t('history.detail.aria')}
      className="@container flex h-full min-w-0 flex-col bg-muted/20"
      data-testid="clipboard-detail"
    >
      <header
        data-tauri-drag-region="deep"
        className="flex h-15 shrink-0 items-center gap-2.5 pl-6 pr-5"
      >
        <span
          className={cn(
            'inline-flex h-6 shrink-0 items-center rounded-full px-2.25 text-ui-caption font-semibold',
            KIND_TINT[kind]
          )}
        >
          {t(`history.type.${kind}`)}
        </span>
        <span className="min-w-0 flex-1 truncate text-ui-body text-muted-foreground">
          {source?.label && (
            <Trans
              i18nKey="history.detail.from"
              values={{ device: source.label }}
              components={{
                strong: <strong className="font-semibold text-foreground" />,
              }}
            />
          )}
        </span>
        <button
          type="button"
          aria-pressed={isFavorited}
          aria-label={t(isFavorited ? 'history.detail.unpin' : 'history.detail.pin')}
          title={t(isFavorited ? 'history.detail.unpin' : 'history.detail.pin')}
          data-tauri-drag-region="false"
          onClick={onToggleFavorite}
          className={cn(
            iconButton,
            isFavorited
              ? 'border-orange-500/30 bg-orange-500/10 text-orange-500'
              : 'border-border bg-background text-foreground hover:bg-muted/60'
          )}
        >
          <Pin className={cn('size-3.75', isFavorited && 'fill-current')} aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label={t('clipboard.actionBar.delete')}
          title={t('clipboard.actionBar.delete')}
          data-tauri-drag-region="false"
          onClick={onDelete}
          className={cn(
            iconButton,
            'border-border bg-background text-foreground hover:bg-destructive/10 hover:text-destructive'
          )}
        >
          <Trash2 className="size-3.75" aria-hidden="true" />
        </button>
      </header>

      {/* The body scrolls on its own so the header and footer stay pinned. */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex min-h-full w-full flex-col gap-4.5 px-6 py-5">
          {state.effectiveStatus === 'transferring' && state.transfer?.status === 'active' && (
            <TransferProgressBar
              progress={state.transfer}
              variant="compact"
              onCancel={cancel}
              cancelling={cancelling}
            />
          )}
          <div
            className={cn(
              'relative overflow-hidden rounded-[0.875rem] bg-card',
              // The dark code block carries its own edge, as in the design.
              kind !== 'code' && 'border border-border/60',
              isImage && 'bg-muted',
              compactBox ? 'min-h-42.5 flex-1' : 'min-h-40 flex-1'
            )}
          >
            {fillsBox ? (
              <div className="absolute inset-0">{content}</div>
            ) : (
              <div className="absolute inset-0">
                <ScrollArea className="h-full [&_[data-slot=scroll-area-viewport]>div]:!block">
                  <div className="min-h-full">{content}</div>
                </ScrollArea>
              </div>
            )}
            {isImage && imageDescriptor && (
              <div className="absolute bottom-2.5 right-2.5 flex gap-1">
                <div role="group" aria-label={t('history.detail.imageZoom')} className="flex gap-1">
                  {(['fit', 'actual'] as const).map(mode => (
                    <button
                      key={mode}
                      type="button"
                      aria-pressed={imageActions.mode === mode}
                      onClick={() => imageActions.setMode(mode)}
                      className={cn(
                        'h-6.5 rounded-[0.4375rem] px-2.25 text-ui-caption text-white transition-colors',
                        imageActions.mode === mode ? 'bg-black/85' : 'bg-black/60 hover:bg-black/75'
                      )}
                    >
                      {t(mode === 'fit' ? 'history.detail.imageFit' : 'history.detail.imageActual')}
                    </button>
                  ))}
                </div>
                <button
                  type="button"
                  onClick={() => imageActions.setQuickLook(true)}
                  className="inline-flex h-6.5 items-center gap-1.25 rounded-[0.4375rem] bg-black/60 px-2.25 text-ui-caption text-white transition-colors hover:bg-black/75"
                >
                  {t('history.detail.quickLook')}
                  <kbd aria-hidden="true" className="font-mono">
                    ␣
                  </kbd>
                </button>
              </div>
            )}
          </div>
          {tagging && item.userTagIds && (
            <HistoryDetailTags tagIds={item.userTagIds} {...tagging} />
          )}
          <div
            className={cn(
              'grid shrink-0 gap-2.5',
              GRID_COLS[cards.length],
              // Under 480px three cards are too narrow for their values; wrap to two.
              cards.length > 2 && '@max-[30rem]:grid-cols-2'
            )}
          >
            {cards.map(card => (
              <MetaCard key={card.id} label={card.label} value={card.value} mono={card.mono} />
            ))}
          </div>
          {delivery && <HistoryDetailDelivery delivery={delivery} />}
        </div>
      </div>

      <footer className="flex h-17 shrink-0 items-center gap-2 border-t border-border/60 bg-background pl-6 pr-5">
        <button
          type="button"
          onClick={onCopy}
          disabled={item.isUnavailable}
          className={cn(pillButton, 'bg-foreground text-background hover:bg-foreground/90')}
        >
          {copySuccess && <Check className="size-4" aria-hidden="true" />}
          {copySuccess ? t('clipboard.item.actions.copied') : t('clipboard.actionBar.copy')}
          {/* The real shortcut (useHistoryController), styled like the design's key hint. */}
          {!copySuccess && (
            <kbd aria-hidden="true" className="font-mono text-ui-caption text-background/60">
              C
            </kbd>
          )}
        </button>
        <ClipboardSendMenu
          key={item.id}
          entryId={item.id}
          disabled={item.isUnavailable || (delivery !== null && delivery.source.tag !== 'local')}
          renderTrigger={({ disabled }) => (
            <button
              type="button"
              disabled={disabled}
              className={cn(
                pillButton,
                'border border-border bg-background px-4.5 text-foreground hover:bg-muted/60 @max-[30rem]:px-3.5'
              )}
            >
              {/* Text-only like the design; the icon stands in once the label hides. */}
              <Send className="hidden size-3.5 @max-[30rem]:block" aria-hidden="true" />
              {/* Icon-only under 480px; the label stays as the accessible name. */}
              <span className="@max-[30rem]:sr-only">{t('history.detail.sendToDevice')}</span>
            </button>
          )}
        />
        {isImage && (
          <>
            <button
              type="button"
              disabled={item.isUnavailable || !imageActions.canAct || imageActions.busy}
              onClick={() => void imageActions.saveAs()}
              className={cn(
                pillButton,
                'border border-border bg-background px-4 text-foreground hover:bg-muted/60 @max-[30rem]:px-3.5'
              )}
            >
              <Download className="hidden size-3.5 @max-[30rem]:block" aria-hidden="true" />
              <span className="@max-[30rem]:sr-only">{t('history.detail.saveAs')}</span>
            </button>
            <button
              type="button"
              disabled={item.isUnavailable || !imageActions.canAct || imageActions.busy}
              onClick={() => void imageActions.openExternally()}
              className={cn(
                pillButton,
                'border border-border bg-background px-4 text-foreground hover:bg-muted/60 @max-[48rem]:px-3.5'
              )}
            >
              <ExternalLink className="hidden size-3.5 @max-[48rem]:block" aria-hidden="true" />
              <span className="@max-[48rem]:sr-only">{t('history.detail.openInPreview')}</span>
            </button>
          </>
        )}
        <span className="flex-1" />
        <span className="truncate text-ui-caption text-muted-foreground @max-[30rem]:hidden">
          {t('history.detail.shortcutHint')}
        </span>
      </footer>
      {imageActions.quickLook && (
        <ImageQuickLook src={quickLookSrc} onClose={() => imageActions.setQuickLook(false)} />
      )}
    </section>
  )
}

export default HistoryDetailPanel
