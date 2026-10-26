import { useState } from 'react'
import { createLogger } from '@/lib/logger'

const log = createLogger('copy-full-invitation')

/** Admission material stays in memory and never becomes diagnostic context. */
export function useCopyFullInvitation(invitation: string) {
  const [status, setStatus] = useState<'idle' | 'copied' | 'failed'>('idle')
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(invitation)
      setStatus('copied')
    } catch {
      log.warn({ error_kind: 'clipboard_write_failed' }, 'Failed to copy full invitation')
      setStatus('failed')
    }
  }
  return { status, copy }
}
