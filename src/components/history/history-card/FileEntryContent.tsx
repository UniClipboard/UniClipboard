import { useTranslation } from 'react-i18next'
import type { ClipboardFileItem } from '@/lib/clipboard-entry'
import { formatFileSize } from '@/utils'
import { getFileExtLabel } from './file-glyph-utils'
import FileGlyph from './FileGlyph'

interface FileEntryContentProps {
  item: ClipboardFileItem
}

function FileEntryContent({ item }: FileEntryContentProps) {
  const { t } = useTranslation()
  const count = item.file_names.length
  const name = item.file_names[0] ?? t('history.unknownFile')
  const primarySize = item.file_sizes[0] ?? -1
  const ext = getFileExtLabel(name)
  // Gate the size label on whether ANY size is known, not on a non-zero total —
  // a known combined size of exactly 0 still renders as `0 B` rather than hiding.
  const knownSizes = item.file_sizes.filter(s => s >= 0)
  const totalSize = knownSizes.reduce((a, b) => a + b, 0)
  const meta =
    count > 1
      ? knownSizes.length > 0
        ? `${t('clipboard.preview.filesCount', { count })} · ${formatFileSize(totalSize)}`
        : t('clipboard.preview.filesCount', { count })
      : primarySize >= 0
        ? formatFileSize(primarySize)
        : ''

  return (
    <div className="flex h-full items-center gap-3">
      <FileGlyph ext={ext} stacked={count > 1} />
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium leading-snug text-foreground/85 line-clamp-2 break-all">
          {name}
        </div>
        {meta && (
          <div className="mt-1 text-[11px] tabular-nums text-muted-foreground/55">{meta}</div>
        )}
      </div>
    </div>
  )
}

export default FileEntryContent
