import { Loader2 } from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useClipboardPreview } from './useClipboardPreview'
import VirtualizedText from '@/components/clipboard/VirtualizedText'

interface ClipboardPreviewPaneProps {
  entryId: string | null
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const ClipboardPreviewPane: React.FC<ClipboardPreviewPaneProps> = ({ entryId }) => {
  const { t } = useTranslation(undefined, { keyPrefix: 'previewPanel' })
  const { preview, loading, error } = useClipboardPreview(entryId)
  const isMac = useMemo(() => navigator.platform.toUpperCase().includes('MAC'), [])

  const isLargeText =
    preview?.contentType === 'text' &&
    preview.textContent != null &&
    preview.textContent.length > 50_000

  return (
    <div className="flex h-screen w-[360px] min-w-[360px] max-w-[360px] flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl">
      <div className="flex items-center justify-between border-b border-border/50 px-3 py-2">
        <span className="text-[12px] font-medium text-foreground">{t('title')}</span>
        {preview && (
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {formatBytes(preview.sizeBytes)}
          </span>
        )}
      </div>

      <div className={isLargeText ? 'flex-1 min-h-0 px-3 py-2' : 'flex-1 overflow-auto px-3 py-2'}>
        {loading ? (
          <div className="flex h-full items-center justify-center" aria-live="polite">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        ) : error ? (
          <div className="flex h-full items-center justify-center text-[12px] text-destructive">
            {t('error')}
          </div>
        ) : preview ? (
          preview.contentType === 'image' ? (
            <div className="flex h-full items-center justify-center">
              {preview.imageUrl ? (
                <img
                  src={preview.imageUrl}
                  className="max-h-full max-w-full rounded-md object-contain"
                  alt={t('imageAlt')}
                />
              ) : (
                <span className="text-[12px] text-muted-foreground">{t('imageUnavailable')}</span>
              )}
            </div>
          ) : isLargeText ? (
            <VirtualizedText text={preview.textContent!} className="h-full" />
          ) : (
            <pre className="cursor-text whitespace-pre-wrap break-words font-mono text-[12px] leading-relaxed text-foreground select-text">
              {preview.textContent}
            </pre>
          )
        ) : (
          <div className="flex h-full items-center justify-center text-[12px] text-muted-foreground">
            {t('empty')}
          </div>
        )}
      </div>

      <div className="flex items-center justify-start border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
        <span>{t('deleteHint', { modifier: isMac ? '⌥' : 'Alt+' })}</span>
      </div>
    </div>
  )
}

export default ClipboardPreviewPane
