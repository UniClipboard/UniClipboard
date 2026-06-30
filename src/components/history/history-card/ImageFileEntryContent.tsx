import type { ClipboardFileItem } from '@/lib/clipboard-entry'
import { formatFileSize } from '@/utils'
import { getFileExtLabel } from './file-glyph-utils'
import FileGlyph from './FileGlyph'
import { useResourceImageUrl } from './useResourceImageUrl'

interface ImageFileEntryContentProps {
  item: ClipboardFileItem
  entryId: string
}

function ImageFileEntryContent({ item, entryId }: ImageFileEntryContentProps) {
  const imageUrl = useResourceImageUrl(entryId)
  const name = item.file_names[0] ?? ''
  const primarySize = item.file_sizes[0] ?? -1

  return (
    <div className="flex h-full items-center gap-3">
      {imageUrl ? (
        <img
          src={imageUrl}
          alt=""
          className="size-12 shrink-0 rounded-md object-cover ring-1 ring-black/5 dark:ring-white/10"
        />
      ) : (
        <FileGlyph ext={getFileExtLabel(name)} />
      )}
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium leading-snug text-foreground/85 line-clamp-2 break-all">
          {name}
        </div>
        {primarySize >= 0 && (
          <div className="mt-1 text-[11px] tabular-nums text-muted-foreground/55">
            {formatFileSize(primarySize)}
          </div>
        )}
      </div>
    </div>
  )
}

export default ImageFileEntryContent
