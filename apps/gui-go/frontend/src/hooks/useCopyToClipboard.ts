import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from '@/components/ui/toast'
import { playUiSound } from '@/lib/ui-sound'

/**
 * Write text to the system clipboard with a transient "copied" flag.
 *
 * Collapses the near-identical copy handlers that the device detail panels
 * used to reimplement (peer-id rows, credential rows, the credentials backup
 * button): each did `writeText` → set a boolean → clear it after ~1.5s →
 * toast on failure. `copy` resolves to whether the write succeeded so callers
 * can branch (e.g. stop before opening a follow-up view).
 */
export function useCopyToClipboard(resetMs = 1500) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    },
    []
  )

  const copy = useCallback(
    async (value: string): Promise<boolean> => {
      try {
        await navigator.clipboard.writeText(value)
        playUiSound('success')
        setCopied(true)
        if (timerRef.current) clearTimeout(timerRef.current)
        timerRef.current = setTimeout(() => setCopied(false), resetMs)
        return true
      } catch {
        toast.error(t('devices.panel.profile.copyFailed'))
        return false
      }
    },
    [resetMs, t]
  )

  return { copied, copy }
}
