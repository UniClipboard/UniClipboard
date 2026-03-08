import { ArrowUp, ArrowDown, Check } from 'lucide-react'
import React from 'react'
import { useAppSelector } from '@/store/hooks'
import { selectAllTransfers } from '@/store/slices/transferSlice'

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const TransferProgressBar: React.FC = () => {
  const transfers = useAppSelector(selectAllTransfers)

  if (transfers.length === 0) {
    return null
  }

  return (
    <div className="mx-3 mt-2 mb-1 flex flex-col gap-1.5">
      {transfers.map(transfer => {
        const percent =
          transfer.totalChunks > 0
            ? Math.round((transfer.chunksCompleted / transfer.totalChunks) * 100)
            : 0
        const isComplete = transfer.chunksCompleted === transfer.totalChunks
        const peerLabel = transfer.peerId.length > 8 ? transfer.peerId.slice(0, 8) : transfer.peerId

        return (
          <div
            key={transfer.transferId}
            className="flex items-center gap-2 rounded-md bg-card text-card-foreground border border-border/50 px-3 py-2"
          >
            {/* Direction icon */}
            <div className="flex-shrink-0">
              {isComplete ? (
                <Check className="h-4 w-4 text-green-500" />
              ) : transfer.direction === 'Sending' ? (
                <ArrowUp className="h-4 w-4 text-blue-500" />
              ) : (
                <ArrowDown className="h-4 w-4 text-green-500" />
              )}
            </div>

            {/* Peer label */}
            <span className="text-xs text-muted-foreground flex-shrink-0 w-16 truncate">
              {peerLabel}
            </span>

            {/* Progress bar */}
            <div className="flex-1 h-2 rounded-full bg-primary/20 overflow-hidden">
              <div
                className="h-full rounded-full bg-primary transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            </div>

            {/* Percentage */}
            <span className="text-xs font-medium text-foreground w-10 text-right flex-shrink-0">
              {percent}%
            </span>

            {/* Bytes transferred */}
            {transfer.totalBytes > 0 && (
              <span className="text-xs text-muted-foreground flex-shrink-0">
                {formatBytes(transfer.bytesTransferred)} / {formatBytes(transfer.totalBytes)}
              </span>
            )}
          </div>
        )
      })}
    </div>
  )
}

export { TransferProgressBar }
