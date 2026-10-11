import { useTranslation } from 'react-i18next'
import { PreviewContent } from '@/components/clipboard/ClipboardPreview'
import { useHistoryImageActions } from '@/components/history/detail/useHistoryImageActions'
import { describeSource } from '@/components/history/history-card/history-card-utils'
import { formatCopiedAt } from '@/components/history/list/history-list-format'
import { historyKind } from '@/components/history/list/history-list-kind'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type { ClipboardImageItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { imageFormatLabel } from '@/lib/image-handoff'
import { formatShortcutHint } from '@/lib/shortcut-format'
import {
  ActionBar,
  Button,
  ContentUnavailable,
  FactList,
  KindBadge,
  PreviewFrame,
  QuickLook,
  SegmentedControl,
  Text,
} from '@/ui-kit'
import { formatFileSize } from '@/utils'
import { HistoryKitDelivery } from './HistoryKitDelivery'
import { HistoryKitDetailTags, type HistoryKitTagging } from './HistoryKitDetailTags'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

interface HistoryKitDetailProps {
  item: DisplayClipboardItem | null
  /** Nothing was ever copied: the detail explains where items come from (design MA1). */
  emptyHistory?: boolean
  tagging: HistoryKitTagging | null
  /** The loaded image entries around this one, for Quick Look's previous / next. */
  imageNeighbors: { previous?: () => void; next?: () => void; position: number; total: number }
  copySuccess: boolean
  onCopy: () => void
  onTogglePin: () => void
  onDelete: () => void
}

/**
 * The detail pane of the selected entry (design MA2): its kind and origin with the pin and delete
 * commands, the content, its tags, delivery to each paired device, and the floating action bar
 * with Copy and Send. Editing an entry is not offered: the Engine has no way to change one.
 */
export function HistoryKitDetail({
  item,
  emptyHistory,
  tagging,
  imageNeighbors,
  copySuccess,
  onCopy,
  onTogglePin,
  onDelete,
}: HistoryKitDetailProps) {
  const { t, i18n } = useTranslation()
  const state = useClipboardPreviewState(item)
  const { delivery } = useEntryDelivery(item?.id ?? null)
  const imageDescriptor =
    state.preview?.contentType === 'image' ? (state.preview.imageBlobPath ?? null) : null
  const imageActions = useHistoryImageActions(
    item?.id,
    imageDescriptor,
    item?.type === 'image' && !item.isUnavailable,
    true
  )
  // Cached by path, so this is the object URL the preview already shows.
  const quickLookSrc = useBlobImageObjectUrl(imageDescriptor, imageActions.quickLook)

  if (!item) {
    return (
      <div className="flex min-h-0 flex-1 flex-col" data-testid="clipboard-detail">
        {emptyHistory ? (
          <ContentUnavailable
            icon="copy"
            title={t('history.kit.nothingCopied')}
            description={t('history.kit.nothingCopiedText')}
          >
            <Text role="caption" tone="secondary">
              {t('history.kit.encryptionNote')}
            </Text>
          </ContentUnavailable>
        ) : (
          <ContentUnavailable
            title={t('history.kit.noSelection')}
            description={t('history.detail.emptyHint')}
          />
        )}
      </div>
    )
  }

  const kind = historyKind(item)
  const isImage = item.type === 'image'
  const image = isImage ? (item.content as ClipboardImageItem | null) : null
  const dims =
    state.imageDimensions ??
    (image && image.width > 0 ? { width: image.width, height: image.height } : null)
  const bytes = image && image.size > 0 ? image.size : (state.preview?.sizeBytes ?? 0)
  const format = imageActions.formatLabel ?? imageFormatLabel(state.preview?.mimeType)
  const imageFacts = [
    ...(dims
      ? [
          {
            id: 'dims',
            label: t('clipboard.preview.dimensions'),
            value: `${dims.width} × ${dims.height}`,
            monospace: true,
          },
        ]
      : []),
    {
      id: 'format',
      label: t('history.kit.format'),
      value: [format, bytes > 0 ? formatFileSize(bytes) : null].filter(Boolean).join(' · ') || '—',
    },
    {
      id: 'copied',
      label: t('history.detail.copied'),
      value: formatCopiedAt(item.activeTime, i18n.language),
    },
  ]
  const pinned = item.isFavorited === true
  const source = delivery ? describeSource(delivery.source, t) : null
  const origin = [source?.label, formatCopiedAt(item.activeTime, i18n.language)]
    .filter(Boolean)
    .join(' · ')
  const sendable = !item.isUnavailable && (delivery === null || delivery.source.tag === 'local')

  return (
    <div
      className="relative flex min-h-0 flex-1 flex-col"
      data-testid="clipboard-detail"
      data-entry-id={item.id}
    >
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 pb-28 pt-1">
        <div className="flex shrink-0 items-center gap-2">
          <KindBadge kind={kind} label={t(`history.type.${kind}`)} variant="pill" />
          <span className="min-w-0 flex-1">
            <Text role="callout" tone="secondary" truncate>
              {origin}
            </Text>
          </span>
          <Button
            variant="subtle"
            icon="pin"
            label={t(pinned ? 'history.detail.unpin' : 'history.detail.pin')}
            tooltip={`${t(pinned ? 'history.detail.unpin' : 'history.detail.pin')} (${formatShortcutHint('mod+p')})`}
            onClick={onTogglePin}
          />
          <Button
            variant="subtle"
            icon="delete"
            label={t('clipboard.contextMenu.delete')}
            onClick={onDelete}
          />
        </div>
        <PreviewFrame
          kind={isImage ? 'image' : kind === 'code' ? 'code' : 'plain'}
          topEnd={
            isImage && imageDescriptor ? (
              <SegmentedControl
                label={t('history.detail.imageZoom')}
                value={imageActions.mode}
                onValueChange={mode => imageActions.setMode(mode as 'fit' | 'actual')}
                items={[
                  { value: 'fit', label: t('history.detail.imageFit') },
                  { value: 'actual', label: t('history.detail.imageActual') },
                ]}
              />
            ) : undefined
          }
          bottomStart={
            isImage && imageDescriptor ? (
              <Button size="small" onClick={() => imageActions.setQuickLook(true)}>
                {`${t('history.detail.quickLook')} ␣`}
              </Button>
            ) : undefined
          }
        >
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
        </PreviewFrame>
        {isImage && <FactList label={t('history.kit.imageFacts')} facts={imageFacts} />}
        {tagging && item.userTagIds && (
          <HistoryKitDetailTags tagIds={item.userTagIds} tagging={tagging} />
        )}
        {delivery && <HistoryKitDelivery delivery={delivery} />}
      </div>
      <ActionBar
        hint={t('history.kit.actionHint', {
          copy: formatShortcutHint('enter'),
          pin: formatShortcutHint('mod+p'),
        })}
      >
        <Button
          variant="primary"
          size="xlarge"
          icon="copy"
          disabled={item.isUnavailable}
          onClick={onCopy}
        >
          {copySuccess
            ? t('clipboard.item.actions.copied')
            : isImage
              ? t('history.kit.copyImage')
              : t('clipboard.contextMenu.copy')}
        </Button>
        {isImage ? (
          <>
            <Button
              size="large"
              disabled={item.isUnavailable || !imageActions.canAct || imageActions.busy}
              onClick={() => void imageActions.saveAs()}
            >
              {t('history.detail.saveAs')}
            </Button>
            <Button
              size="large"
              disabled={item.isUnavailable || !imageActions.canAct || imageActions.busy}
              onClick={() => void imageActions.openExternally()}
            >
              {t('history.detail.openInPreview')}
            </Button>
          </>
        ) : (
          <HistoryKitSendMenu entryIds={[item.id]} disabled={!sendable} />
        )}
      </ActionBar>
      <QuickLook
        open={imageActions.quickLook}
        onOpenChange={imageActions.setQuickLook}
        title={t('history.kit.quickLookPosition', {
          position: imageNeighbors.position,
          total: imageNeighbors.total,
        })}
        closeLabel={t('history.detail.quickLookClose')}
        src={quickLookSrc}
        alt={t('clipboard.item.altText.clipboardImage')}
        previous={
          imageNeighbors.previous && {
            label: t('history.kit.previousImage'),
            onClick: imageNeighbors.previous,
          }
        }
        next={
          imageNeighbors.next && { label: t('history.kit.nextImage'), onClick: imageNeighbors.next }
        }
        primary={{
          label: t('history.kit.copyAndClose'),
          onClick: () => {
            onCopy()
            imageActions.setQuickLook(false)
          },
        }}
        actions={
          <Button
            size="small"
            disabled={!imageActions.canAct || imageActions.busy}
            onClick={() => void imageActions.openExternally()}
          >
            {t('history.detail.openInPreview')}
          </Button>
        }
      />
    </div>
  )
}
