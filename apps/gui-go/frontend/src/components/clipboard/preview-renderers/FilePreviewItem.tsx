import { File, FileArchive, FileAudio, FileVideo, Image as ImageIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { convertFileSrc } from '@/host/core'
import { isImageFileName } from '@/lib/clipboard-utils'
import { cn } from '@/lib/utils'
import { formatFileSize } from '@/utils'

interface FilePreviewItemProps {
  name: string
  size?: number
  path?: string | null
  imageUrl: string | null
  layout: 'row' | 'single' | 'tile'
  children?: ReactNode
}

export default function FilePreviewItem({
  name,
  size,
  path,
  imageUrl,
  layout,
  children,
}: FilePreviewItemProps) {
  const isImage = isImageFileName(name)
  const src = isImage ? (imageUrl ?? (path ? convertFileSrc(path) : null)) : null
  const extensionStart = name.lastIndexOf('.')
  const extension = extensionStart > 0 ? name.slice(extensionStart) : ''
  const stem = extension ? name.slice(0, extensionStart) : name
  const ext = extension.toLowerCase()
  const Icon = isImage
    ? ImageIcon
    : /\.(zip|rar|7z|tar|gz)$/.test(ext)
      ? FileArchive
      : /\.(mp4|mov|avi|mkv|webm)$/.test(ext)
        ? FileVideo
        : /\.(mp3|wav|flac|aac)$/.test(ext)
          ? FileAudio
          : File
  const large = layout === 'tile' || (layout === 'single' && !!src)

  return (
    <li
      className={cn(
        'min-w-0',
        large || children ? 'flex flex-col gap-3' : 'flex items-center gap-3 px-4 py-3',
        children && !large && 'px-4 py-3',
        layout === 'single' && large && 'p-4'
      )}
    >
      <div
        className={cn(
          children
            ? large
              ? 'flex min-w-0 flex-col gap-3'
              : 'flex min-w-0 items-center gap-3'
            : 'contents'
        )}
      >
        <div
          className={cn(
            'relative flex shrink-0 items-center justify-center overflow-hidden rounded-lg bg-muted/40 text-muted-foreground',
            large ? 'aspect-[4/3] w-full' : 'size-10'
          )}
        >
          <Icon aria-hidden="true" className={large ? 'size-10' : 'size-5'} />
          {src && (
            <img
              key={src}
              src={src}
              alt={name}
              loading="lazy"
              className={cn(
                'absolute inset-0 size-full',
                large ? 'object-contain' : 'object-cover'
              )}
              onError={event => {
                event.currentTarget.hidden = true
              }}
            />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div
            title={name}
            className={cn(
              'text-ui-body font-medium',
              layout === 'single' ? 'break-all' : 'flex min-w-0'
            )}
          >
            {layout === 'single' ? (
              name
            ) : (
              <>
                <span className="truncate">{stem}</span>
                <span className="shrink-0">{extension}</span>
              </>
            )}
          </div>
          {size != null && size >= 0 && (
            <div className="mt-1 text-ui-caption tabular-nums text-muted-foreground">
              {formatFileSize(size)}
            </div>
          )}
        </div>
      </div>
      {children}
    </li>
  )
}
