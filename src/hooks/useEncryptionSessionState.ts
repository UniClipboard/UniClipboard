import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { getEncryptionSessionStatus } from '@/api/security'

interface EncryptionSessionState {
  encryptionReady: boolean
  isLocked: boolean
}

export function useEncryptionSessionState(): EncryptionSessionState {
  const [state, setState] = useState<EncryptionSessionState>({
    encryptionReady: false,
    isLocked: false,
  })

  useEffect(() => {
    let cancelled = false

    const syncState = async () => {
      try {
        const status = await getEncryptionSessionStatus()
        if (cancelled) return

        const ready = !status.initialized || status.session_ready
        setState({
          encryptionReady: ready,
          isLocked: status.initialized && !status.session_ready,
        })
      } catch (err) {
        if (cancelled) return
        console.error('Failed to check encryption session status:', err)
        setState({ encryptionReady: true, isLocked: false })
      }
    }

    const unlistenPromise = listen<'SessionReady' | { type?: string }>(
      'encryption://event',
      event => {
        const eventType = typeof event.payload === 'string' ? event.payload : event.payload?.type
        if (eventType === 'SessionReady' && !cancelled) {
          setState({ encryptionReady: true, isLocked: false })
        }
      }
    )

    void syncState()

    return () => {
      cancelled = true
      unlistenPromise.then(fn => fn())
    }
  }, [])

  return state
}
