import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  CloudOff,
  Database,
  File,
  Hash,
  Image as ImageIcon,
  Layers,
  Loader2,
} from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import type { DisplayClipboardItem } from '../ClipboardContent'
import { cn } from '@/lib/utils'
import type { EntryTransferStatus, TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'

interface FilePreviewProps {
  effectiveStatus: EntryTransferStatus['status'] | undefined
  entryStatus: EntryTransferStatus | undefined
  item: DisplayClipboardItem
  transfer: TransferProgressInfo | undefined
}

function getFileExt(name: string) {
  return name.split('.').pop()?.toLowerCase() ?? ''
}

function getFileIcon(name: string) {
  const ext = getFileExt(name)
  if (['jpg', 'jpeg', 'png', 'gif', 'bmp', 'svg', 'webp'].includes(ext)) return ImageIcon
  if (['mp4', 'mov', 'avi', 'mkv', 'webm'].includes(ext)) return Layers
  if (['mp3', 'wav', 'flac', 'aac'].includes(ext)) return Hash
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return Database
  return File
}

const FilePreview: React.FC<FilePreviewProps> = ({
  effectiveStatus,
  entryStatus,
  item,
  transfer,
}) => {
  const { t } = useTranslation()
  const fileItem = item.content

  if (!fileItem || !('file_names' in fileItem)) {
    return null
  }

  const fileNames = fileItem.file_names
  const fileSizes = fileItem.file_sizes
  const isSingleFile = fileNames.length === 1
  const percent =
    transfer && transfer.totalBytes && transfer.totalBytes > 0
      ? Math.round((transfer.bytesTransferred / transfer.totalBytes) * 100)
      : transfer?.totalChunks && transfer.totalChunks > 0
        ? Math.round((transfer.chunksCompleted / transfer.totalChunks) * 100)
        : 0

  const renderStatusBadge = () => (
    <div className="flex flex-wrap gap-2">
      {effectiveStatus === 'pending' && (
        <div className="flex items-center gap-1.5 rounded-full bg-muted/30 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-muted-foreground backdrop-blur-md ring-1 ring-border/20">
          <Clock size={10} />
          {t('clipboard.transfer.pending')}
        </div>
      )}
      {effectiveStatus === 'transferring' && (
        <div className="flex items-center gap-1.5 rounded-full bg-primary/15 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-primary backdrop-blur-md ring-1 ring-primary/30">
          <Loader2 size={10} className="animate-spin" />
          {t('clipboard.transfer.transferring')}
        </div>
      )}
      {effectiveStatus === 'failed' && (
        <div className="flex items-center gap-1.5 rounded-full bg-destructive/15 px-2.5 py-1 text-[10px] font-bold tracking-wider text-destructive backdrop-blur-md ring-1 ring-destructive/30">
          <AlertTriangle size={10} />
          <span>{t('clipboard.transfer.failed')}</span>
        </div>
      )}
      {effectiveStatus === 'completed' && (
        <div className="flex items-center gap-1.5 rounded-full bg-green-500/15 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-green-500 backdrop-blur-md ring-1 ring-green-500/30">
          <CheckCircle2 size={10} />
          {t('clipboard.transfer.completed')}
        </div>
      )}
      {!effectiveStatus && item.isDownloaded === false && (
        <div className="flex items-center gap-1.5 rounded-full bg-orange-500/10 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-orange-500 backdrop-blur-md ring-1 ring-orange-500/20">
          <CloudOff size={10} />
          {t('clipboard.preview.notDownloaded')}
        </div>
      )}
    </div>
  )

  const renderProgressOverlay = (isHero = false) => {
    if (effectiveStatus !== 'transferring' || !transfer) return null

    return (
      <div
        className={cn(
          'absolute inset-0 z-0 transition-[width] duration-300 ease-out',
          isHero ? 'bg-primary/5' : 'bg-primary/8'
        )}
        style={{ width: `${percent}%` }}
      />
    )
  }

  if (isSingleFile) {
    return (
      <div className="flex h-full flex-col items-center justify-center space-y-10 p-8">
        <div className="relative w-full max-w-sm">
          <div className="relative overflow-hidden rounded-[2rem] border border-border/40 bg-background/60 p-1 ring-1 ring-white/10">
            {renderProgressOverlay(true)}

            <div className="relative z-10 flex flex-col items-center p-10 text-center">
              <div className="relative mb-8">
                <div className="relative flex h-24 w-24 items-center justify-center rounded-[1.75rem] bg-gradient-to-br from-primary/10 to-primary/5 text-primary">
                  {React.createElement(getFileIcon(fileNames[0]), { size: 40 })}
                </div>
              </div>

              <div className="mb-8 w-full space-y-2">
                <h3 className="truncate px-4 text-lg font-semibold tracking-tight text-foreground/90">
                  {fileNames[0]}
                </h3>
                <div className="flex items-center justify-center gap-2 text-sm font-medium text-muted-foreground">
                  {fileSizes[0] >= 0 && (
                    <span className="tabular-nums">{formatFileSize(fileSizes[0])}</span>
                  )}
                  {item.device && (
                    <>
                      <span className="text-xs opacity-20">•</span>
                      <span className="text-xs uppercase tracking-tighter opacity-70">
                        {item.device}
                      </span>
                    </>
                  )}
                </div>
              </div>

              {renderStatusBadge()}
            </div>
          </div>
        </div>

        {effectiveStatus === 'failed' && entryStatus?.reason && (
          <div className="flex max-w-sm items-start gap-2 rounded-xl border border-destructive/10 bg-destructive/5 px-4 py-3 text-xs text-destructive/80">
            <AlertTriangle size={14} className="mt-0.5 shrink-0" />
            <span>{entryStatus.reason}</span>
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-6 p-6">
      <div className="flex items-center justify-between">
        {renderStatusBadge()}
        {item.device && (
          <div className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground/50">
            {t('clipboard.preview.sourceDevice')}: {item.device}
          </div>
        )}
      </div>

      <div className="grid gap-3">
        {fileNames.map((name, index) => (
          <div
            key={index}
            className="relative overflow-hidden rounded-2xl border border-border/30 bg-muted/10 p-4"
          >
            {renderProgressOverlay()}

            <div className="relative z-10 flex items-center gap-4">
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-background/50 text-muted-foreground/60">
                {React.createElement(getFileIcon(name), { size: 18 })}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-foreground/80">{name}</div>
                {fileSizes[index] != null && (
                  <div className="mt-0.5 text-[11px] font-medium tabular-nums text-muted-foreground/60">
                    {formatFileSize(fileSizes[index])}
                  </div>
                )}
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}

export default FilePreview
