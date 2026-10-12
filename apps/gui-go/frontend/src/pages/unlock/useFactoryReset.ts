import { useState } from 'react'
import { isFactoryResetError, resetSpace, type FactoryResetError } from '@/api/security'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('factory-reset-dialog')
const CONFIRMATION_TOKEN = 'RESET'

function errorI18nKey(error: FactoryResetError): string {
  switch (error.code) {
    case 'KEY_MATERIAL_WIPE_FAILED':
      return 'unlock.factoryReset.errors.keyMaterialWipeFailed'
    case 'STORAGE_FAILED':
      return 'unlock.factoryReset.errors.storageFailed'
    case 'RESTART_REQUIRED':
      return 'unlock.factoryReset.errors.restartRequired'
    case 'FACADE_UNAVAILABLE':
      return 'unlock.factoryReset.errors.facadeUnavailable'
    case 'INTERNAL':
      return 'unlock.factoryReset.errors.internal'
  }
}

/** The typed "RESET" confirmation and the reset itself, shared by every skin of the dialog. */
export function useFactoryReset(onClose: () => void, onResetSucceeded?: () => void) {
  const [confirmation, setConfirmation] = useState('')
  const [resetting, setResetting] = useState(false)
  const [restarting, setRestarting] = useState(false)
  const [errorKey, setErrorKey] = useState<string | null>(null)
  const confirmed = confirmation.trim() === CONFIRMATION_TOKEN
  const busy = resetting || restarting

  const close = () => {
    if (busy) return
    onClose()
    setConfirmation('')
    setErrorKey(null)
  }

  const submit = async () => {
    if (!confirmed || resetting) return
    setResetting(true)
    setErrorKey(null)
    try {
      await resetSpace()
      try {
        await refreshSetupState()
      } catch (error) {
        log.warn({ err: error }, 'Setup state refresh failed after reset')
      }
      onClose()
      setConfirmation('')
      onResetSucceeded?.()
    } catch (error) {
      if (isFactoryResetError(error)) {
        if (error.code === 'RESTART_REQUIRED') {
          // Key material is already wiped on the daemon side; the daemon's
          // Engine runtime itself failed to rebuild in place. Restarting
          // only the GUI leaves that daemon process (and its broken Engine)
          // running, so this must restart the daemon first — mirroring the
          // existing restartDaemon() -> restartApp() sequence used after a
          // config import that also requires a fresh Engine on next boot.
          setRestarting(true)
          try {
            await commands.restartDaemon()
            await commands.restartApp()
          } catch (restartError) {
            log.error(
              { err: restartError },
              'Failed to restart daemon after factory-reset runtime rebuild failure'
            )
            setRestarting(false)
            setErrorKey(errorI18nKey(error))
          }
          return
        }
        setErrorKey(errorI18nKey(error))
      } else {
        log.error({ err: error }, 'Unexpected factory reset error')
        setErrorKey('unlock.factoryReset.errors.internal')
      }
    } finally {
      setResetting(false)
    }
  }

  return {
    confirmation,
    setConfirmation,
    confirmed,
    resetting,
    restarting,
    busy,
    errorKey,
    close,
    submit,
  }
}
