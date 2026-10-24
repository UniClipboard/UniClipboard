import { useCallback, useState } from 'react'
import { cancelEntryReceive, cancelFileTransfer } from '@/api/file_transfer'
import { reportError } from '@/observability/errors'
import { useAppDispatch } from '@/store/hooks'
import { markReceiveCancellationIncomplete } from '@/store/slices/fileTransferSlice'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'

/** Cancel the active transfer of an entry: an inbound receive attempt when it
 * has one, otherwise the transfer itself. `cancelling` guards double clicks.
 *
 * A `cancellation_incomplete` answer means the Engine is still releasing output
 * or cleanup: the receive is recorded as still stopping, never as cancelled, and
 * the final state arrives through the usual attempt events. */
export function useCancelEntryTransfer(
  itemId: string | undefined,
  transfer: TransferProgressInfo | undefined
) {
  const dispatch = useAppDispatch()
  const [cancelling, setCancelling] = useState(false)
  const transferId = transfer?.transferId
  const attemptId = transfer?.attemptId
  const cancel = useCallback(async () => {
    if (!transferId || cancelling) return
    setCancelling(true)
    try {
      if (itemId && attemptId) {
        const outcome = await cancelEntryReceive(itemId, attemptId)
        if (outcome === 'cancellation_incomplete') {
          dispatch(markReceiveCancellationIncomplete({ entryId: itemId, attemptId }))
        }
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
  }, [attemptId, cancelling, dispatch, itemId, transferId])
  return { cancelling, cancel }
}
