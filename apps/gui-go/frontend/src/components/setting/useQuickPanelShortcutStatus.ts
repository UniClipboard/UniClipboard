import { useEffect, useState } from 'react'
import { listen } from '@/host/event'
import { commands, type QuickPanelShortcutStatus } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'

const log = createLogger('quick-panel-shortcut-status')

/** Host event carrying the global shortcut's apply status (ADR-013 section 10). */
const SHORTCUT_STATUS_EVENT = 'quick-panel://shortcut-status'

/**
 * The quick panel shortcut's apply status as the host holds it: whether the saved binding is the
 * one the OS holds. Read once, then followed through the host event, since the status can change
 * without any settings read (an external write, a helper restart).
 */
export function useQuickPanelShortcutStatus(): QuickPanelShortcutStatus | null {
  const [status, setStatus] = useState<QuickPanelShortcutStatus | null>(null)
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    void listen<QuickPanelShortcutStatus>(SHORTCUT_STATUS_EVENT, event => {
      if (!cancelled) setStatus(event.payload)
    }).then(fn => {
      if (cancelled) fn()
      else unlisten = fn
    })
    commands
      .getQuickPanelShortcutStatus()
      .then(value => {
        if (!cancelled) setStatus(current => current ?? value)
      })
      .catch((err: unknown) => log.warn({ err }, 'Failed to read the quick panel shortcut status'))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
  return status
}
