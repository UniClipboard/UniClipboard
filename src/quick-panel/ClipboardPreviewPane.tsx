import { File, Loader2 } from 'lucide-react'
import React, { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useClipboardPreview } from './useClipboardPreview'
import EntryDeliverySection from '@/components/clipboard/EntryDeliverySection'
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
  const { preview, loading, error, delivery } = useClipboardPreview(entryId)
  const isMac = useMemo(() => navigator.platform.toUpperCase().includes('MAC'), [])

  const isLargeText =
    preview?.contentType === 'text' &&
    preview.textContent != null &&
    preview.textContent.length > 50_000

  return (
    <div className="flex h-full w-full min-w-0 flex-col overflow-hidden rounded-xl border border-border/50 bg-background/95 shadow-xl backdrop-blur-xl">
      <div className="flex items-center justify-between border-b border-border/50 px-3 py-2">
        <span className="text-[12px] font-medium text-foreground">{t('title')}</span>
        {preview && (
          <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
            {formatBytes(preview.sizeBytes)}
          </span>
        )}
      </div>

      <div
        className={
          isLargeText ? 'flex-1 min-h-0 px-3 py-2' : 'scrollbar-thin flex-1 overflow-auto px-3 py-2'
        }
      >
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
          ) : preview.contentType === 'file' && preview.fileNames ? (
            <div className="flex flex-col gap-2">
              {preview.fileNames.map((name, i) => (
                <div key={i} className="flex items-center gap-2 text-[12px] text-foreground">
                  <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  <span className="truncate">{name}</span>
                </div>
              ))}
            </div>
          ) : isLargeText ? (
            <VirtualizedText text={preview.textContent!} className="scrollbar-thin h-full" />
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

      <EntryDeliverySection delivery={delivery} compact />

      <div className="flex items-center justify-start border-t border-border/50 px-3 py-1.5 text-[11px] text-muted-foreground">
        <span>{t('deleteHint', { modifier: isMac ? '⌥' : 'Alt+' })}</span>
      </div>
    </div>
  )
}

export default ClipboardPreviewPane
