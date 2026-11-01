import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { isImageFileName } from '@/lib/clipboard-utils'
import type { EntryTransferStatus, TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'
import FilePreviewItem from './FilePreviewItem'
import FilePreviewStatus from './FilePreviewStatus'

interface FilePreviewProps {
  effectiveStatus: EntryTransferStatus['status'] | undefined
  entryStatus: EntryTransferStatus | undefined
  item: DisplayClipboardItem
  preview: ClipboardPreviewData | null
  transfer: TransferProgressInfo | undefined
  summary?: string
  children?: ReactNode
  renderFileProgress?: (index: number, name: string) => ReactNode
}

export default function FilePreview({
  item,
  preview,
  effectiveStatus,
  entryStatus,
  summary,
  children,
  renderFileProgress,
}: FilePreviewProps) {
  const { t } = useTranslation()
  const imageUrl = useBlobImageObjectUrl(
    preview?.contentType === 'image' ? (preview.imageBlobPath ?? null) : null
  )
  const files = item.content
  if (!files || !('file_names' in files)) return null
  const count = files.file_names.length
  const gallery = count > 1 && files.file_names.every(isImageFileName)
  const totalKnown = files.file_sizes.length === count && files.file_sizes.every(size => size >= 0)
  const total = totalKnown ? files.file_sizes.reduce((sum, size) => sum + size, 0) : null

  return (
    <section className="min-w-0" aria-label={t('clipboard.preview.filesCount', { count })}>
      <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border/50 px-4 py-3">
        <div className="flex flex-wrap items-baseline gap-2">
          <span className="text-ui-body font-medium">
            {t('clipboard.preview.filesCount', { count })}
          </span>
          {count > 1 && total !== null && (
            <span className="text-ui-caption text-muted-foreground">{formatFileSize(total)}</span>
          )}
        </div>
        {summary ? (
          <span className="text-ui-caption text-muted-foreground" role="status">
            {summary}
          </span>
        ) : (
          <FilePreviewStatus status={effectiveStatus} reason={entryStatus?.reason} />
        )}
      </header>
      {children}
      <ul
        aria-label={gallery ? t('clipboard.preview.imageFilesAriaLabel') : undefined}
        className={
          gallery
            ? 'grid grid-cols-[repeat(auto-fit,minmax(min(100%,11rem),1fr))] gap-4 p-4'
            : 'divide-y divide-border/50'
        }
      >
        {files.file_names.map((name, index) => (
          <FilePreviewItem
            key={`${name}-${index}`}
            name={name}
            size={files.file_sizes[index]}
            path={files.file_missing?.[index] ? null : files.file_paths?.[index]}
            imageUrl={count === 1 ? imageUrl : null}
            layout={gallery ? 'tile' : count === 1 ? 'single' : 'row'}
          >
            {renderFileProgress?.(index, name)}
          </FilePreviewItem>
        ))}
      </ul>
    </section>
  )
}
