import { Clipboard } from 'lucide-react'
import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cancelEntryReceive, cancelFileTransfer } from '@/api/file_transfer'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type {
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { linkItemFromTextContent } from '@/lib/clipboard-utils'
import { reportError } from '@/observability/errors'
import ClipboardPreviewInfo from './ClipboardPreviewInfo'
import CodePreview from './preview-renderers/CodePreview'
import FilePreview from './preview-renderers/FilePreview'
import ImagePreview from './preview-renderers/ImagePreview'
import LinkPreview from './preview-renderers/LinkPreview'
import TextPreview from './preview-renderers/TextPreview'
import { isLargeTextPreview } from './preview-renderers/textPreviewUtils'
import TransferProgressBar from './TransferProgressBar'

interface ClipboardPreviewProps {
  item: DisplayClipboardItem | null
  actions?: React.ReactNode
}

interface PreviewContentProps {
  item: DisplayClipboardItem
  loading: boolean
  preview: ReturnType<typeof useClipboardPreviewState>['preview']
  effectiveStatus: ReturnType<typeof useClipboardPreviewState>['effectiveStatus']
  entryStatus: ReturnType<typeof useClipboardPreviewState>['entryStatus']
  transfer: ReturnType<typeof useClipboardPreviewState>['transfer']
  setImageDimensions: ReturnType<typeof useClipboardPreviewState>['setImageDimensions']
}

const PreviewContent: React.FC<PreviewContentProps> = ({
  item,
  loading,
  preview,
  effectiveStatus,
  entryStatus,
  transfer,
  setImageDimensions,
}) => {
  const { t } = useTranslation()
  const textItem = item.content as ClipboardTextItem | null
  const hasCodeTag = item.contentTags?.includes('code') ?? false
  const hasLinkTag = item.contentTags?.includes('link') ?? false
  switch (item.type) {
    case 'text': {
      if (hasCodeTag && textItem) {
        return (
          <CodePreview
            item={{ code: textItem.display_text, char_count: textItem.char_count }}
            preview={preview}
          />
        )
      }
      if (hasLinkTag && textItem) {
        const linkItem = linkItemFromTextContent(textItem)
        if (linkItem) return <LinkPreview item={linkItem} />
      }
      return (
        <TextPreview item={item.content as ClipboardTextItem} loading={loading} preview={preview} />
      )
    }
    case 'richtext': {
      return (
        <TextPreview item={item.content as ClipboardTextItem} loading={loading} preview={preview} />
      )
    }
    case 'image': {
      return (
        <ImagePreview
          item={item.content as ClipboardImageItem}
          loading={loading}
          preview={preview}
          setImageDimensions={setImageDimensions}
        />
      )
    }
    case 'file': {
      return (
        <FilePreview
          effectiveStatus={effectiveStatus}
          entryStatus={entryStatus}
          item={item}
          preview={preview}
          transfer={transfer}
        />
      )
    }
    default:
      return (
        <div className="p-8 text-center font-medium italic text-muted-foreground opacity-40">
          {t('clipboard.item.unknownContent')}
        </div>
      )
  }
}

const ClipboardPreview: React.FC<ClipboardPreviewProps> = ({ item, actions }) => {
  const { t } = useTranslation()
  const {
    effectiveStatus,
    entryStatus,
    imageDimensions,
    loading,
    preview,
    setImageDimensions,
    transfer,
  } = useClipboardPreviewState(item)
  const { delivery } = useEntryDelivery(item?.id ?? null)
  const [cancelling, setCancelling] = useState(false)

  const itemId = item?.id
  const transferId = transfer?.transferId
  const attemptId = transfer?.attemptId
  const handleCancelTransfer = useCallback(async () => {
    if (!transferId || cancelling) return
    setCancelling(true)
    try {
      if (itemId && attemptId) {
        await cancelEntryReceive(itemId, attemptId)
      } else {
        await cancelFileTransfer(transferId)
      }
    } catch (err) {
      reportError(err, {
        command: itemId && attemptId ? 'cancelEntryReceive' : 'cancelFileTransfer',
        transferId,
      })
    } finally {
      // 无论成功或失败都释放本地锁，避免后续 transfer 被误禁用。
      setCancelling(false)
    }
  }, [attemptId, cancelling, itemId, transferId])

  if (!item) {
    return (
      <div className="flex h-full flex-1 min-h-0 flex-col items-center justify-center gap-3 bg-card text-muted-foreground">
        <Clipboard className="size-10 text-muted-foreground/20" />
        <span className="text-sm font-medium opacity-50">{t('clipboard.preview.selectItem')}</span>
      </div>
    )
  }

  const isLargeText =
    (item.type === 'text' || item.type === 'richtext') &&
    isLargeTextPreview(item.content as ClipboardTextItem, preview, loading)
  // Code renders as an editor-like pane that fills the available height and owns
  // its own scrolling, so it skips the auto-height ScrollArea wrapper.
  const fillsParent = isLargeText || item.contentTags?.includes('code') === true

  const content = (
    <PreviewContent
      item={item}
      loading={loading}
      preview={preview}
      effectiveStatus={effectiveStatus}
      entryStatus={entryStatus}
      transfer={transfer}
      setImageDimensions={setImageDimensions}
    />
  )

  return (
    <div className="flex h-full flex-1 min-h-0 flex-col bg-card" data-testid="clipboard-detail">
      <ClipboardPreviewInfo
        item={item}
        preview={preview}
        imageDimensions={imageDimensions}
        delivery={delivery}
      />

      <div className="relative flex-1 min-h-0">
        {fillsParent ? (
          <div className="absolute inset-0">{content}</div>
        ) : (
          <ScrollArea className="h-full [&_[data-slot=scroll-area-viewport]>div]:!block">
            <div className="min-h-full">{content}</div>
          </ScrollArea>
        )}
      </div>

      {(effectiveStatus === 'transferring' || actions) && (
        <div className="flex min-h-[64px] shrink-0 items-center justify-between bg-card px-6 py-4">
          <div className="mr-8 min-w-0 flex-1">
            {effectiveStatus === 'transferring' && transfer && transfer.status === 'active' && (
              <div className="max-w-[280px]">
                <TransferProgressBar
                  progress={transfer}
                  variant="compact"
                  onCancel={handleCancelTransfer}
                  cancelling={cancelling}
                />
              </div>
            )}
          </div>
          {actions && <div className="shrink-0">{actions}</div>}
        </div>
      )}
    </div>
  )
}

export default ClipboardPreview
