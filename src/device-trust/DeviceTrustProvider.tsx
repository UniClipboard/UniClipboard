import { getCurrentWindow } from '@tauri-apps/api/window'
import { onAction } from '@tauri-apps/plugin-notification'
import { useCallback, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  decideDeviceTrust as submitDecision,
  getDeviceTrust,
  type DeviceTrustChoice,
  type DeviceTrustSnapshot,
} from '@/api/daemon/device-trust'
import { usePlatform } from '@/hooks/usePlatform'
import { daemonWs } from '@/lib/daemon-ws'
import { DeviceTrustContext } from './device-trust-context'
import {
  DEVICE_TRUST_NOTIFICATION_ID,
  notifyDeviceTrustSnapshot,
} from './device-trust-notifications'

export function DeviceTrustProvider({
  enabled,
  children,
}: {
  enabled: boolean
  children: ReactNode
}) {
  const { isTauri } = usePlatform()
  const [snapshot, setSnapshot] = useState<DeviceTrustSnapshot | null>(null)
  const [loading, setLoading] = useState(false)
  const [decisionBusy, setDecisionBusy] = useState(false)
  const [decisionError, setDecisionError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    if (!enabled) return
    setLoading(true)
    try {
      const next = await getDeviceTrust()
      setSnapshot(next)
      setDecisionError(null)
      await notifyDeviceTrustSnapshot(next)
      if (next.currentChange && isTauri) {
        try {
          const window = getCurrentWindow()
          await window.show()
          await window.unminimize()
          await window.setFocus()
        } catch {
          // The blocking modal is already rendered even if the OS rejects focus.
        }
      }
    } catch (error) {
      setDecisionError(error instanceof Error ? error.message : String(error))
    } finally {
      setLoading(false)
    }
  }, [enabled, isTauri])

  useEffect(() => {
    if (!enabled) return
    void refresh()
    const unsubscribe = daemonWs.subscribe(['device-trust'], () => void refresh())
    const onVisible = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      unsubscribe()
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [enabled, refresh])

  useEffect(() => {
    if (!enabled || !isTauri) return
    let disposed = false
    let unlisten: (() => void) | null = null
    onAction(async notification => {
      if (notification.id !== DEVICE_TRUST_NOTIFICATION_ID) return
      try {
        const window = getCurrentWindow()
        await window.show()
        await window.unminimize()
        await window.setFocus()
      } catch {
        // The modal remains available the next time the main window is shown.
      }
    })
      .then(listener => {
        if (disposed) listener.unregister()
        else unlisten = () => listener.unregister()
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [enabled, isTauri])

  const decide = useCallback(
    async (choice: DeviceTrustChoice, confirmLocalRemoval: boolean) => {
      const changeId = snapshot?.currentChange?.changeId
      if (!changeId || decisionBusy) return
      setDecisionBusy(true)
      setDecisionError(null)
      try {
        const result = await submitDecision(changeId, choice, confirmLocalRemoval)
        setSnapshot(result.snapshot)
        if (result.kind === 'state_changed') setDecisionError('device_state_changed')
      } catch (error) {
        await refresh()
        setDecisionError(error instanceof Error ? error.message : String(error))
      } finally {
        setDecisionBusy(false)
      }
    },
    [decisionBusy, refresh, snapshot]
  )

  const value = useMemo(
    () => ({ snapshot, loading, decisionBusy, decisionError, refresh, decide }),
    [snapshot, loading, decisionBusy, decisionError, refresh, decide]
  )
  return <DeviceTrustContext value={value}>{children}</DeviceTrustContext>
}
