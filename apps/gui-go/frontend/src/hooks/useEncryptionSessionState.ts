import { useEffect, useRef, useState } from 'react'
import { getEncryptionState } from '@/api/daemon'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'

const log = createLogger('use-encryption-session-state')

interface EncryptionSessionState {
  encryptionReady: boolean
  isLocked: boolean
}

const ENCRYPTION_STATE_POLL_INTERVAL_MS = 5_000

export function useEncryptionSessionState(): EncryptionSessionState {
  const [state, setState] = useState<EncryptionSessionState>({
    encryptionReady: false,
    isLocked: false,
  })
  const hasSuccessfulSyncRef = useRef(false)

  useEffect(() => {
    let cancelled = false

    const applyStatus = (status: { initialized: boolean; sessionReady: boolean }) => {
      hasSuccessfulSyncRef.current = true
      const ready = !status.initialized || status.sessionReady
      setState({
        encryptionReady: ready,
        isLocked: status.initialized && !status.sessionReady,
      })
    }

    const syncState = async () => {
      try {
        const status = await getEncryptionState()
        if (cancelled) return

        applyStatus(status)
      } catch (err) {
        if (cancelled) return
        log.warn({ err }, 'Failed to check encryption session status')
        if (!hasSuccessfulSyncRef.current) {
          setState({ encryptionReady: false, isLocked: false })
        }
      }
    }

    const handler = (event: { eventType: string }) => {
      if (cancelled) return
      if (event.eventType === 'encryption.session_ready') {
        hasSuccessfulSyncRef.current = true
        setState({ encryptionReady: true, isLocked: false })
      }
    }

    const unsubscribe = daemonWs.subscribe(['encryption'], handler)
    const intervalId = window.setInterval(() => {
      void syncState()
    }, ENCRYPTION_STATE_POLL_INTERVAL_MS)

    void syncState()

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
      unsubscribe()
    }
  }, [])

  return state
}
