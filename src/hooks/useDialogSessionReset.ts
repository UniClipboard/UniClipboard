import { useCallback, useReducer } from 'react'

/** Reset form state only after the popup has finished closing. */
export function useDialogSessionReset() {
  const [sessionKey, resetSession] = useReducer((key: number) => key + 1, 0)
  const onOpenChangeComplete = useCallback((open: boolean) => {
    if (!open) resetSession()
  }, [])
  return { sessionKey, onOpenChangeComplete }
}
