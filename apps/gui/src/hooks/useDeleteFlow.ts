import { useCallback, useRef, useState } from 'react'
import {
  readDeleteConfirmationEnabled,
  setDeleteConfirmationEnabled,
} from '@/lib/delete-confirmation-preference'

/**
 * Delete flow for the history grid: the saved preference optionally gates
 * removal behind a confirm dialog, then a brief "deleting" window drives the
 * card's exit animation before the entry is dropped. The caller supplies the
 * already error-handled async removal.
 */
export function useDeleteFlow(remove: (id: string) => Promise<void>, animateMs = 400) {
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const targetRef = useRef<string | null>(null)

  const startDelete = useCallback(
    (targetId: string) => {
      setDeleteDialogOpen(false)
      setDeletingId(targetId)
      // Defer the removal so the exit animation can play first.
      setTimeout(async () => {
        await remove(targetId)
        setDeletingId(null)
        targetRef.current = null
      }, animateMs)
    },
    [remove, animateMs]
  )

  const requestDelete = useCallback(
    (id: string) => {
      targetRef.current = id
      if (readDeleteConfirmationEnabled()) {
        setDeleteDialogOpen(true)
        return
      }
      startDelete(id)
    },
    [startDelete]
  )

  const confirmDelete = useCallback(
    (skipFutureConfirmation = false) => {
      const targetId = targetRef.current
      if (!targetId) return
      if (skipFutureConfirmation) setDeleteConfirmationEnabled(false)
      startDelete(targetId)
    },
    [startDelete]
  )

  return { deleteDialogOpen, setDeleteDialogOpen, deletingId, requestDelete, confirmDelete }
}
