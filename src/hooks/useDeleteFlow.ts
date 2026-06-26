import { useCallback, useRef, useState } from 'react'

/**
 * Two-step delete for the history grid: a confirm dialog gates the removal, and
 * a brief "deleting" window drives the card's exit animation before the entry is
 * dropped. The caller supplies the (already error-handled) async removal.
 */
export function useDeleteFlow(remove: (id: string) => Promise<void>, animateMs = 400) {
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deletingId, setDeletingId] = useState<string | null>(null)
  const targetRef = useRef<string | null>(null)

  const requestDelete = useCallback((id: string) => {
    targetRef.current = id
    setDeleteDialogOpen(true)
  }, [])

  const confirmDelete = useCallback(() => {
    const targetId = targetRef.current
    if (!targetId) return
    setDeletingId(targetId)
    // Defer the removal so the exit animation can play first.
    setTimeout(async () => {
      await remove(targetId)
      setDeletingId(null)
      targetRef.current = null
    }, animateMs)
  }, [remove, animateMs])

  return { deleteDialogOpen, setDeleteDialogOpen, deletingId, requestDelete, confirmDelete }
}
