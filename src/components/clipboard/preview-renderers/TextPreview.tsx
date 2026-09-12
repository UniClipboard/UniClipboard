import { Loader2 } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { ClipboardTextItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import VirtualizedText from '../VirtualizedText'
import { getTextPreviewContent, LARGE_TEXT_THRESHOLD } from './textPreviewUtils'

interface TextPreviewProps {
  item: ClipboardTextItem
  loading: boolean
  preview: ClipboardPreviewData | null
}

const TextPreview: React.FC<TextPreviewProps> = ({ item, loading, preview }) => {
  const { t } = useTranslation()
  const displayText = getTextPreviewContent(item, preview)

  if (!loading && displayText.length > LARGE_TEXT_THRESHOLD) {
    return (
      <div className="h-full px-[var(--clipboard-preview-inset,0rem)]">
        <VirtualizedText text={displayText} className="selectable h-full" />
      </div>
    )
  }

  return (
    <div className="px-[var(--clipboard-preview-inset,1.5rem)] py-6">
      {loading ? (
        <div className="flex items-center gap-2 text-muted-foreground/60">
          <Loader2 className="size-4 animate-spin" />
          <span className="text-ui-body font-medium">{t('clipboard.item.loading')}</span>
        </div>
      ) : (
        <p className="selectable break-all whitespace-pre-wrap font-mono text-ui-body text-foreground/80">
          {displayText}
        </p>
      )}
    </div>
  )
}

export default TextPreview
