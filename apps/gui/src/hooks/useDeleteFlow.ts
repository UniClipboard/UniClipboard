import { useCallback, useRef, useState } from 'react'
import {
  readDeleteConfirmationEnabled,
  setDeleteConfirmationEnabled,
} from '@/lib/delete-confirmation-preference'

const NONE: ReadonlySet<string> = new Set()

/**
 * Delete flow for the history grid: the saved preference optionally gates
 * removal behind a confirm dialog, then a brief "deleting" window drives the
 * rows' exit animation before the entries are dropped. One request may carry
 * several ids (the list's bulk selection). The caller supplies the already
 * error-handled async removal.
 */
export function useDeleteFlow(remove: (id: string) => Promise<void>, animateMs = 400) {
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false)
  const [deletingIds, setDeletingIds] = useState<ReadonlySet<string>>(NONE)
  // The ids the open dialog would delete; drives its count.
  const [targetIds, setTargetIds] = useState<readonly string[]>([])
  const targetRef = useRef<readonly string[]>([])

  const startDelete = useCallback(
    (ids: readonly string[]) => {
      setDeleteDialogOpen(false)
      setDeletingIds(new Set(ids))
      // Defer the removal so the exit animation can play first.
      setTimeout(async () => {
        await Promise.all(ids.map(id => remove(id)))
        setDeletingIds(NONE)
        targetRef.current = []
      }, animateMs)
    },
    [remove, animateMs]
  )

  const requestDelete = useCallback(
    (idOrIds: string | readonly string[]) => {
      const ids = typeof idOrIds === 'string' ? [idOrIds] : idOrIds
      if (ids.length === 0) return
      targetRef.current = ids
      setTargetIds(ids)
      if (readDeleteConfirmationEnabled()) {
        setDeleteDialogOpen(true)
        return
      }
      startDelete(ids)
    },
    [startDelete]
  )

  const confirmDelete = useCallback(
    (skipFutureConfirmation = false) => {
      const ids = targetRef.current
      if (ids.length === 0) return
      if (skipFutureConfirmation) setDeleteConfirmationEnabled(false)
      startDelete(ids)
    },
    [startDelete]
  )

  return {
    deleteDialogOpen,
    setDeleteDialogOpen,
    deletingIds,
    deleteCount: targetIds.length,
    requestDelete,
    confirmDelete,
  }
}
