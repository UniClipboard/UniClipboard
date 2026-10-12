import { useState } from 'react'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('unlock-page')

/**
 * The unlock page's flow: an explicit keyring attempt first, the space passphrase only when the
 * keyring cannot unlock. Content visibility stays with the caller (`onUnlockSucceeded`).
 */
export function useUnlockFlow(onUnlockSucceeded?: () => void) {
  const [needsPassphrase, setNeedsPassphrase] = useState(false)
  const [unlocking, setUnlocking] = useState(false)

  const finishUnlock = async () => {
    try {
      await ensureSetupRealtimeSync()
      await refreshSetupState()
    } catch (error) {
      log.warn({ err: error }, 'Setup state refresh failed after unlock')
    }
    onUnlockSucceeded?.()
  }

  const unlock = async () => {
    if (unlocking) return
    setUnlocking(true)
    try {
      if (await commands.unlockContentFromKeyring()) {
        await finishUnlock()
        return
      }
    } catch (error) {
      log.warn({ err: error }, 'Keyring unlock failed; requesting the passphrase')
    } finally {
      setUnlocking(false)
    }
    setNeedsPassphrase(true)
  }

  return { needsPassphrase, unlocking, unlock, finishUnlock }
}
