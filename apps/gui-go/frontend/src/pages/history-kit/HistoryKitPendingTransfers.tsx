import FilePreviewProgress from '@/components/clipboard/preview-renderers/FilePreviewProgress'
import { useAppSelector } from '@/store/hooks'

/** A LAN upload has progress before SyncClipboard apply assigns its history entry. */
export function HistoryKitPendingTransfers() {
  const transfers = useAppSelector(state => state.fileTransfer.activeTransfers)
  const pending = Object.values(transfers).filter(
    transfer =>
      transfer.status === 'active' && transfer.direction === 'receiving' && !transfer.entryId
  )
  if (pending.length === 0) return null
  return (
    <div className="shrink-0">
      {pending.map(transfer => (
        <FilePreviewProgress key={transfer.transferId} transfer={transfer} />
      ))}
    </div>
  )
}
