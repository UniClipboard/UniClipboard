import { useCallback, useState } from 'react'
import { cancelEntryReceive, cancelFileTransfer } from '@/api/file_transfer'
import { reportError } from '@/observability/errors'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'

/** Cancel the active transfer of an entry: an inbound receive attempt when it
 * has one, otherwise the transfer itself. `cancelling` guards double clicks. */
export function useCancelEntryTransfer(
  itemId: string | undefined,
  transfer: TransferProgressInfo | undefined
) {
  const [cancelling, setCancelling] = useState(false)
  const transferId = transfer?.transferId
  const attemptId = transfer?.attemptId
  const cancel = useCallback(async () => {
    if (!transferId || cancelling) return
    setCancelling(true)
    try {
      if (itemId && attemptId) {
        await cancelEntryReceive(itemId, attemptId)
      } else {
        await cancelFileTransfer(transferId)
      }
    } catch (err) {
      reportError(err, {
        command: itemId && attemptId ? 'cancelEntryReceive' : 'cancelFileTransfer',
        transferId,
      })
    } finally {
      // Release the local lock either way so later transfers are not blocked.
      setCancelling(false)
    }
  }, [attemptId, cancelling, itemId, transferId])
  return { cancelling, cancel }
}
