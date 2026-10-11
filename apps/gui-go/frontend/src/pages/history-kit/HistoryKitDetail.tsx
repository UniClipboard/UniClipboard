import { useTranslation } from 'react-i18next'
import { PreviewContent } from '@/components/clipboard/ClipboardPreview'
import { describeSource } from '@/components/history/history-card/history-card-utils'
import { formatCopiedAt } from '@/components/history/list/history-list-format'
import { historyKind } from '@/components/history/list/history-list-kind'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import { formatShortcutHint } from '@/lib/shortcut-format'
import { ActionBar, Button, ContentUnavailable, KindBadge, Text } from '@/ui-kit'
import { HistoryKitDelivery } from './HistoryKitDelivery'
import { HistoryKitDetailTags, type HistoryKitTagging } from './HistoryKitDetailTags'
import { HistoryKitSendMenu } from './HistoryKitSendMenu'

interface HistoryKitDetailProps {
  item: DisplayClipboardItem | null
  /** Nothing was ever copied: the detail explains where items come from (design MA1). */
  emptyHistory?: boolean
  tagging: HistoryKitTagging | null
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
  copySuccess,
  onCopy,
  onTogglePin,
  onDelete,
}: HistoryKitDetailProps) {
  const { t, i18n } = useTranslation()
  const state = useClipboardPreviewState(item)
  const { delivery } = useEntryDelivery(item?.id ?? null)

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
  const pinned = item.isFavorited === true
  const source = delivery ? describeSource(delivery.source, t) : null
  const origin = [source?.label, formatCopiedAt(item.activeTime, i18n.language)]
    .filter(Boolean)
    .join(' · ')
  const sendable = !item.isUnavailable && (delivery === null || delivery.source.tag === 'local')

  return (
    <div className="relative flex min-h-0 flex-1 flex-col" data-testid="clipboard-detail">
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
        <div className="relative min-h-40 shrink-0 overflow-hidden rounded-xl">
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
          />
        </div>
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
          {copySuccess ? t('clipboard.item.actions.copied') : t('clipboard.contextMenu.copy')}
        </Button>
        <HistoryKitSendMenu entryIds={[item.id]} disabled={!sendable} />
      </ActionBar>
    </div>
  )
}
