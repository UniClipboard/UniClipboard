import { useCallback, useRef, useState } from 'react'
import type { UpdateMetadata } from '@/api/updater'

type PendingUpdateAction = () => void | Promise<void>

export function useUpdateConfirmationGate(
  update: UpdateMetadata | null,
  confirmUpdate: () => Promise<void>
) {
  const [open, setOpen] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const pendingActionRef = useRef<PendingUpdateAction | null>(null)

  const request = useCallback(
    (action: PendingUpdateAction) => {
      const status = update?.confirmation.status
      if (status === 'not_required' || status === 'confirmed') {
        void action()
        return
      }

      pendingActionRef.current = status === 'pending' ? action : null
      setOpen(true)
    },
    [update]
  )

  const handleOpenChange = useCallback((nextOpen: boolean) => {
    setOpen(nextOpen)
    if (!nextOpen) pendingActionRef.current = null
  }, [])

  const confirmAndContinue = useCallback(async () => {
    if (update?.confirmation.status !== 'pending') return

    setConfirming(true)
    try {
      await confirmUpdate()
      const action = pendingActionRef.current
      pendingActionRef.current = null
      setOpen(false)
      await action?.()
    } finally {
      setConfirming(false)
    }
  }, [confirmUpdate, update])

  return {
    confirmationDialogOpen: open,
    confirmationInProgress: confirming,
    requestUpdateAction: request,
    setConfirmationDialogOpen: handleOpenChange,
    confirmAndContinue,
  }
}
