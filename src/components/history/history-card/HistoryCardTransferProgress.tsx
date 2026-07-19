import { cn } from '@/lib/utils'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { formatFileSize } from '@/utils'

interface HistoryCardTransferProgressProps {
  isFileType: boolean
  isTransferring: boolean
  transfer?: TransferProgressInfo
  percent: number
  /** When true (directory sends), suppress the byte-progress bar and byte text —
   * their percentage is meaningless — and show a constant full-width activity
   * tint instead. Status is conveyed by the header label. */
  hideByteProgress: boolean
}

function HistoryCardTransferProgress({
  isFileType,
  isTransferring,
  transfer,
  percent,
  hideByteProgress,
}: HistoryCardTransferProgressProps) {
  if (!isFileType) return null

  const active = isTransferring && !!transfer

  return (
    <>
      <div
        className={cn(
          'pointer-events-none absolute inset-0 z-0 bg-primary/8 transition-all duration-500 ease-out',
          active ? 'opacity-100' : 'opacity-0'
        )}
        // Directory sends have no meaningful percentage, so the tint stays
        // full-width to signal activity without a fake fill level.
        style={{ width: active && !hideByteProgress ? `${percent}%` : '100%' }}
      />
      {!hideByteProgress && (
        <div
          className={cn(
            'pointer-events-none absolute bottom-1.5 left-3.5 right-3.5 z-10 flex items-center gap-1.5 transition-opacity duration-500 ease-out',
            active ? 'opacity-100' : 'opacity-0'
          )}
        >
          {transfer && (
            <>
              <div className="h-px flex-1 overflow-hidden rounded-full bg-primary/15">
                <div
                  className="h-full bg-primary/40 transition-[width] duration-300 ease-out"
                  style={{ width: `${percent}%` }}
                />
              </div>
              <span className="shrink-0 text-[9px] tabular-nums text-primary/50">
                {transfer.totalBytes
                  ? `${formatFileSize(transfer.bytesTransferred)} / ${formatFileSize(transfer.totalBytes)}`
                  : formatFileSize(transfer.bytesTransferred)}
              </span>
            </>
          )}
        </div>
      )}
    </>
  )
}

export default HistoryCardTransferProgress
