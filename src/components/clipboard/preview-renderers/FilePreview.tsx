import { convertFileSrc } from '@tauri-apps/api/core'
import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  Database,
  File,
  Hash,
  Image as ImageIcon,
  Layers,
  Loader2,
  Images,
  XCircle,
} from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import type { DisplayClipboardItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { isImageFileName } from '@/lib/clipboard-utils'
import { cn } from '@/lib/utils'
import {
  type EntryTransferStatus,
  normalizeCancelReason,
  type TransferProgressInfo,
} from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'

/** 已知 cancel reason 子原因白名单。后端 wire 上送来的字符串与这里枚举
 * 一致时,UI 用 `clipboard.transfer.cancelReason.<reason>` 渲染中文文案;
 * 否则 fallback 到通用 `cancelReason.unknown`。 */
const KNOWN_CANCEL_REASONS = new Set([
  'local_user',
  'remote_peer',
  'replaced',
  'timeout',
  'unknown',
])

interface FilePreviewProps {
  effectiveStatus: EntryTransferStatus['status'] | undefined
  entryStatus: EntryTransferStatus | undefined
  item: DisplayClipboardItem
  preview: ClipboardPreviewData | null
  transfer: TransferProgressInfo | undefined
}

function getFileExt(name: string) {
  return name.split('.').pop()?.toLowerCase() ?? ''
}

function getCancelReasonText(
  t: ReturnType<typeof useTranslation>['t'],
  reason: string | null | undefined
): string {
  const normalized = normalizeCancelReason(reason)
  if (normalized && KNOWN_CANCEL_REASONS.has(normalized)) {
    return t(`clipboard.transfer.cancelReason.${normalized}`)
  }
  return t('clipboard.transfer.cancelReason.unknown')
}

interface StatusBadgeProps {
  effectiveStatus: EntryTransferStatus['status'] | undefined
  transfer: TransferProgressInfo | undefined
}

const StatusBadge: React.FC<StatusBadgeProps> = ({ effectiveStatus, transfer }) => {
  const { t } = useTranslation()
  return (
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
          {transfer?.direction === 'Sending'
            ? t('clipboard.transfer.sending')
            : transfer?.direction === 'Receiving'
              ? t('clipboard.transfer.receiving')
              : t('clipboard.transfer.transferring')}
        </div>
      )}
      {effectiveStatus === 'failed' && (
        <div className="flex items-center gap-1.5 rounded-full bg-destructive/15 px-2.5 py-1 text-[10px] font-bold tracking-wider text-destructive backdrop-blur-md ring-1 ring-destructive/30">
          <AlertTriangle size={10} />
          <span>{t('clipboard.transfer.failed')}</span>
        </div>
      )}
      {effectiveStatus === 'cancelled' && (
        <div className="flex items-center gap-1.5 rounded-full bg-muted/40 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-muted-foreground backdrop-blur-md ring-1 ring-border/30">
          <XCircle size={10} />
          <span>{t('clipboard.transfer.cancelled')}</span>
        </div>
      )}
      {effectiveStatus === 'completed' && (
        <div className="flex items-center gap-1.5 rounded-full bg-green-500/15 px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-green-500 backdrop-blur-md ring-1 ring-green-500/30">
          <CheckCircle2 size={10} />
          {t('clipboard.transfer.completed')}
        </div>
      )}
    </div>
  )
}

interface ProgressOverlayProps {
  effectiveStatus: EntryTransferStatus['status'] | undefined
  transfer: TransferProgressInfo | undefined
  percent: number
  isHero?: boolean
}

const ProgressOverlay: React.FC<ProgressOverlayProps> = ({
  effectiveStatus,
  transfer,
  percent,
  isHero = false,
}) => {
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

function getFileIcon(name: string) {
  const ext = getFileExt(name)
  if (['jpg', 'jpeg', 'png', 'gif', 'bmp', 'svg', 'webp'].includes(ext)) return ImageIcon
  if (['mp4', 'mov', 'avi', 'mkv', 'webm'].includes(ext)) return Layers
  if (['mp3', 'wav', 'flac', 'aac'].includes(ext)) return Hash
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return Database
  return File
}

function getKnownTotalSize(fileSizes: number[]): number | null {
  const knownSizes = fileSizes.filter(size => size >= 0)
  if (knownSizes.length === 0) return null
  return knownSizes.reduce((sum, size) => sum + size, 0)
}

function getLocalImageUrl(path: string | null): string | null {
  return path ? convertFileSrc(path) : null
}

function getImageFilePath(itemPaths: (string | null)[] | undefined, index: number): string | null {
  return itemPaths?.[index] ?? null
}

const FilePreview: React.FC<FilePreviewProps> = ({
  effectiveStatus,
  entryStatus,
  item,
  preview,
  transfer,
}) => {
  const { t } = useTranslation()
  const fileItem = item.content

  // Resolve the image preview to a token-free `blob:` object URL up front (hook
  // must run before any early return). Null unless the entry actually has an
  // image descriptor.
  const imageObjectUrl = useBlobImageObjectUrl(
    preview?.contentType === 'image' ? (preview.imageBlobPath ?? null) : null
  )

  if (!fileItem || !('file_names' in fileItem)) {
    return null
  }

  const fileNames = fileItem.file_names
  const fileSizes = fileItem.file_sizes
  const isSingleFile = fileNames.length === 1
  const singleFileName = fileNames[0] ?? ''
  const isImageFileGroup = fileNames.length > 1 && fileNames.every(name => isImageFileName(name))
  // Layout selection is synchronous (descriptor presence); the `<img src>`
  // itself fills in once `imageObjectUrl` resolves, so the hero/grid layout
  // never flickers through the plain file-card while bytes load.
  const hasSingleImage =
    isSingleFile && isImageFileName(singleFileName) && preview?.contentType === 'image'
  const percent =
    transfer && transfer.totalBytes && transfer.totalBytes > 0
      ? Math.round((transfer.bytesTransferred / transfer.totalBytes) * 100)
      : 0

  if (isSingleFile) {
    const title = singleFileName
    const fileMeta = (
      <div className="flex flex-wrap items-center justify-center gap-2 text-sm font-medium text-muted-foreground">
        {fileSizes[0] >= 0 && <span className="tabular-nums">{formatFileSize(fileSizes[0])}</span>}
        {item.device && (
          <>
            <span className="text-xs opacity-20">•</span>
            <span className="text-xs uppercase tracking-tighter opacity-70">{item.device}</span>
          </>
        )}
      </div>
    )

    if (hasSingleImage) {
      return (
        <div className="flex min-h-full flex-col items-center justify-center gap-6 p-8">
          <div className="relative flex w-full max-w-5xl items-center justify-center overflow-hidden rounded-lg bg-muted/10 p-2">
            <ProgressOverlay
              effectiveStatus={effectiveStatus}
              transfer={transfer}
              percent={percent}
              isHero
            />
            {imageObjectUrl ? (
              <img
                src={imageObjectUrl}
                alt={title}
                className="relative z-10 max-h-[60vh] max-w-full rounded-md object-contain shadow-2xl ring-1 ring-black/5 dark:ring-white/10"
              />
            ) : (
              // imageObjectUrl is null while the blob loads and stays null if the
              // fetch fails; show a muted placeholder instead of a broken <img>.
              <div className="relative z-10 flex aspect-[4/3] w-full max-w-md items-center justify-center rounded-md bg-background/40">
                <ImageIcon className="size-12 text-muted-foreground/30" />
              </div>
            )}
          </div>

          <div className="flex w-full max-w-4xl flex-col items-center gap-3 text-center">
            <h3 className="max-w-full break-all text-lg font-semibold leading-snug text-foreground/90">
              {title}
            </h3>
            {fileMeta}
            <StatusBadge effectiveStatus={effectiveStatus} transfer={transfer} />
          </div>

          {effectiveStatus === 'failed' && entryStatus?.reason && (
            <div className="flex max-w-sm items-start gap-2 rounded-xl border border-destructive/10 bg-destructive/5 px-4 py-3 text-xs text-destructive/80">
              <AlertTriangle size={14} className="mt-0.5 shrink-0" />
              <span>{entryStatus.reason}</span>
            </div>
          )}

          {effectiveStatus === 'cancelled' && (
            <div className="flex max-w-sm items-start gap-2 rounded-xl border border-border/20 bg-muted/30 px-4 py-3 text-xs text-muted-foreground">
              <XCircle size={14} className="mt-0.5 shrink-0" />
              <span>{getCancelReasonText(t, entryStatus?.reason)}</span>
            </div>
          )}
        </div>
      )
    }

    return (
      <div className="flex h-full flex-col items-center justify-center gap-y-10 p-8">
        <div className="relative w-full max-w-sm">
          <div className="relative overflow-hidden rounded-[2rem] border border-border/40 bg-background/60 p-1 ring-1 ring-white/10">
            <ProgressOverlay
              effectiveStatus={effectiveStatus}
              transfer={transfer}
              percent={percent}
              isHero
            />

            <div className="relative z-10 flex flex-col items-center p-10 text-center">
              <div className="relative mb-8">
                <div className="relative flex size-24 items-center justify-center rounded-[1.75rem] bg-gradient-to-br from-primary/10 to-primary/5 text-primary">
                  {React.createElement(getFileIcon(title), { size: 40 })}
                </div>
              </div>

              <div className="mb-8 w-full space-y-2">
                <h3 className="break-all px-4 text-lg font-semibold leading-snug text-foreground/90">
                  {title}
                </h3>
                {fileMeta}
              </div>

              <StatusBadge effectiveStatus={effectiveStatus} transfer={transfer} />
            </div>
          </div>
        </div>

        {effectiveStatus === 'failed' && entryStatus?.reason && (
          <div className="flex max-w-sm items-start gap-2 rounded-xl border border-destructive/10 bg-destructive/5 px-4 py-3 text-xs text-destructive/80">
            <AlertTriangle size={14} className="mt-0.5 shrink-0" />
            <span>{entryStatus.reason}</span>
          </div>
        )}

        {effectiveStatus === 'cancelled' && (
          <div className="flex max-w-sm items-start gap-2 rounded-xl border border-border/20 bg-muted/30 px-4 py-3 text-xs text-muted-foreground">
            <XCircle size={14} className="mt-0.5 shrink-0" />
            <span>{getCancelReasonText(t, entryStatus?.reason)}</span>
          </div>
        )}
      </div>
    )
  }

  if (isImageFileGroup) {
    const totalSize = getKnownTotalSize(fileSizes)
    return (
      <div className="space-y-6 p-6">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-2 text-sm font-medium text-muted-foreground">
            <span>{t('clipboard.preview.imagesCount', { count: fileNames.length })}</span>
            {totalSize != null && (
              <>
                <span className="text-xs opacity-25">•</span>
                <span className="tabular-nums">{formatFileSize(totalSize)}</span>
              </>
            )}
          </div>
          <StatusBadge effectiveStatus={effectiveStatus} transfer={transfer} />
        </div>

        <div
          role="list"
          aria-label={t('clipboard.preview.imageFilesAriaLabel')}
          className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4"
        >
          {fileNames.map((name, index) => {
            const thumbnailUrl =
              getLocalImageUrl(getImageFilePath(fileItem.file_paths, index)) ??
              (index === 0 ? imageObjectUrl : null)
            return (
              <div
                key={`${name}-${index}`}
                role="listitem"
                className="relative overflow-hidden rounded-lg border border-border/40 bg-muted/10"
              >
                <ProgressOverlay
                  effectiveStatus={effectiveStatus}
                  transfer={transfer}
                  percent={percent}
                />
                <div className="relative z-10 flex aspect-[4/3] items-center justify-center bg-background/40 p-2">
                  {thumbnailUrl ? (
                    <img
                      src={thumbnailUrl}
                      alt={name}
                      className="max-h-full max-w-full rounded-md object-contain shadow-lg ring-1 ring-black/5 dark:ring-white/10"
                    />
                  ) : (
                    <div className="flex size-16 items-center justify-center rounded-xl bg-primary/10 text-primary/70">
                      <Images size={28} />
                    </div>
                  )}
                </div>
                <div className="space-y-1 p-3">
                  <div className="break-all text-sm font-semibold leading-snug text-foreground/85">
                    {name}
                  </div>
                  {fileSizes[index] != null && fileSizes[index] >= 0 && (
                    <div className="text-[11px] font-medium tabular-nums text-muted-foreground/60">
                      {formatFileSize(fileSizes[index])}
                    </div>
                  )}
                </div>
              </div>
            )
          })}
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-6 p-6">
      <div className="flex items-center justify-between">
        <StatusBadge effectiveStatus={effectiveStatus} transfer={transfer} />
        {item.device && (
          <div className="text-[10px] font-bold uppercase tracking-widest text-muted-foreground/50">
            {t('clipboard.preview.sourceDevice')}: {item.device}
          </div>
        )}
      </div>

      <div className="grid gap-3">
        {fileNames.map((name, index) => (
          <div
            key={`${name}-${index}`}
            className="relative overflow-hidden rounded-2xl border border-border/30 bg-muted/10 p-4"
          >
            <ProgressOverlay
              effectiveStatus={effectiveStatus}
              transfer={transfer}
              percent={percent}
            />

            <div className="relative z-10 flex items-center gap-4">
              <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-background/50 text-muted-foreground/60">
                {React.createElement(getFileIcon(name), { size: 18 })}
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-foreground/80">{name}</div>
                {fileSizes[index] != null && fileSizes[index] >= 0 && (
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
