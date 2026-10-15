import { useState } from 'react'
import { resetSetup } from '@/api/daemon/setupV2'
import { createLogger } from '@/lib/logger'
import { refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('rebuild-space')
const REBUILD_CONFIRMATION_TOKEN = 'RESET'

/**
 * Rebuild Space, confirmed by typing RESET: resets the setup to a new space with only this device.
 * `rebuild` resolves `true` on success; a failure leaves `errorKey` set for the confirmation.
 */
export function useRebuildSpace(onRebuildSucceeded?: () => void) {
  const [confirmation, setConfirmation] = useState('')
  const [rebuilding, setRebuilding] = useState(false)
  const [errorKey, setErrorKey] = useState<string | null>(null)
  const confirmed = confirmation.trim().toUpperCase() === REBUILD_CONFIRMATION_TOKEN

  const rebuild = async (): Promise<boolean> => {
    if (!confirmed || rebuilding) return false
    setRebuilding(true)
    setErrorKey(null)
    try {
      await resetSetup()
      try {
        await refreshSetupState()
      } catch (refreshErr) {
        log.warn({ err: refreshErr }, 'Setup state refresh failed after space rebuild')
      }
      onRebuildSucceeded?.()
      return true
    } catch (error) {
      log.error({ err: error }, 'Space rebuild failed')
      setErrorKey('devices.panel.danger.failed')
      return false
    } finally {
      setRebuilding(false)
    }
  }

  const reset = () => {
    setConfirmation('')
    setErrorKey(null)
  }

  return { confirmation, setConfirmation, confirmed, rebuilding, errorKey, rebuild, reset }
}
