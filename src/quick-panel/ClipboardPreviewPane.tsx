import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import ClipboardPreview from '@/components/clipboard/ClipboardPreview'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'

interface ClipboardPreviewPaneProps {
  item: DisplayClipboardItem | null
}

function ClipboardPreviewPane({ item }: ClipboardPreviewPaneProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'previewPanel' })
  const isMac = useMemo(() => navigator.platform.toUpperCase().includes('MAC'), [])

  return (
    <div className="flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-card text-card-foreground shadow-xl backdrop-blur-xl">
      <div className="min-h-0 flex-1" data-testid="quick-panel-preview-area">
        <ClipboardPreview item={item} />
      </div>

      <div className="flex items-center justify-start border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
        <span>{t('deleteHint', { modifier: isMac ? '⌥' : 'Alt+' })}</span>
      </div>
    </div>
  )
}

export default ClipboardPreviewPane
