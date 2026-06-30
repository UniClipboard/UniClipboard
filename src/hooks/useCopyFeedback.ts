import { useCallback, useRef, useState } from 'react'

/**
 * Transient "just copied" feedback for the history grid. Tracks which entry
 * currently shows the success state (auto-clearing after `resetMs`) and which
 * entry should be promoted to the front of the list after a copy. Owning the
 * auto-clear timer here keeps the page component free of feedback plumbing.
 */
export function useCopyFeedback(resetMs = 1200) {
  const [copySuccessId, setCopySuccessId] = useState<string | null>(null)
  const [promotedId, setPromotedId] = useState<string | null>(null)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const markCopied = useCallback(
    (id: string) => {
      if (timerRef.current) clearTimeout(timerRef.current)
      setCopySuccessId(id)
      setPromotedId(id)
      timerRef.current = setTimeout(() => setCopySuccessId(null), resetMs)
    },
    [resetMs]
  )

  return { copySuccessId, promotedId, markCopied }
}
