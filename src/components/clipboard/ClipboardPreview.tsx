import { Clipboard } from 'lucide-react'
import React, { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cancelFileTransfer } from '@/api/file_transfer'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'
import { useEntryDelivery } from '@/hooks/useEntryDelivery'
import type {
  ClipboardCodeItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
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
  switch (item.type) {
    case 'text': {
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
    case 'link': {
      return <LinkPreview item={item.content as ClipboardLinkItem} />
    }
    case 'code': {
      return <CodePreview item={item.content as ClipboardCodeItem} preview={preview} />
    }
    case 'file': {
      return (
        <FilePreview
          effectiveStatus={effectiveStatus}
          entryStatus={entryStatus}
          item={item}
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

  const transferId = transfer?.transferId
  const handleCancelTransfer = useCallback(async () => {
    if (!transferId || cancelling) return
    setCancelling(true)
    try {
      await cancelFileTransfer(transferId)
    } catch (err) {
      reportError(err, { command: 'cancelFileTransfer', transferId })
    } finally {
      // 无论成功或失败都释放本地锁，避免后续 transfer 被误禁用。
      setCancelling(false)
    }
  }, [transferId, cancelling])

  if (!item) {
    return (
      <div className="flex flex-1 min-h-0 flex-col items-center justify-center gap-3 bg-card text-muted-foreground">
        <Clipboard className="size-10 text-muted-foreground/20" />
        <span className="text-sm font-medium opacity-50">{t('clipboard.preview.selectItem')}</span>
      </div>
    )
  }

  const isLargeText =
    item.type === 'text' && isLargeTextPreview(item.content as ClipboardTextItem, preview, loading)
  // Code renders as an editor-like pane that fills the available height and owns
  // its own scrolling, so it skips the auto-height ScrollArea wrapper.
  const fillsParent = isLargeText || item.type === 'code'

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
    <div className="flex flex-1 min-h-0 flex-col bg-card" data-testid="clipboard-detail">
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
