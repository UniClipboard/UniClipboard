import { Clipboard } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { DisplayClipboardItem } from './ClipboardContent'
import ClipboardPreviewInfo from './ClipboardPreviewInfo'
import CodePreview from './preview-renderers/CodePreview'
import FilePreview from './preview-renderers/FilePreview'
import ImagePreview from './preview-renderers/ImagePreview'
import LinkPreview from './preview-renderers/LinkPreview'
import TextPreview from './preview-renderers/TextPreview'
import { isLargeTextPreview } from './preview-renderers/textPreviewUtils'
import TransferProgressBar from './TransferProgressBar'
import {
  ClipboardCodeItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
} from '@/api/clipboardItems'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useClipboardPreviewState } from '@/hooks/useClipboardPreviewState'

interface ClipboardPreviewProps {
  item: DisplayClipboardItem | null
  actions?: React.ReactNode
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

  if (!item) {
    return (
      <div className="flex flex-1 min-h-0 flex-col items-center justify-center gap-3 bg-muted/5 text-muted-foreground">
        <Clipboard className="h-10 w-10 text-muted-foreground/20" />
        <span className="text-sm font-medium opacity-50">{t('clipboard.preview.selectItem')}</span>
      </div>
    )
  }

  const renderContent = () => {
    switch (item.type) {
      case 'text': {
        return (
          <TextPreview
            item={item.content as ClipboardTextItem}
            loading={loading}
            preview={preview}
          />
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

  const isLargeText =
    item.type === 'text' && isLargeTextPreview(item.content as ClipboardTextItem, preview, loading)

  return (
    <div className="flex flex-1 min-h-0 flex-col bg-background/20 backdrop-blur-sm">
      <ClipboardPreviewInfo item={item} preview={preview} imageDimensions={imageDimensions} />

      <div className="relative flex-1 min-h-0">
        {isLargeText ? (
          <div className="absolute inset-0">{renderContent()}</div>
        ) : (
          <ScrollArea className="h-full [&_[data-slot=scroll-area-viewport]>div]:!block">
            <div className="min-h-full">{renderContent()}</div>
          </ScrollArea>
        )}
      </div>

      {(effectiveStatus === 'transferring' || actions) && (
        <div className="flex min-h-[64px] shrink-0 items-center justify-between bg-background/40 px-6 py-4 backdrop-blur-xl">
          <div className="mr-8 min-w-0 flex-1">
            {effectiveStatus === 'transferring' && transfer && transfer.status === 'active' && (
              <div className="max-w-[280px]">
                <TransferProgressBar progress={transfer} variant="compact" />
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
