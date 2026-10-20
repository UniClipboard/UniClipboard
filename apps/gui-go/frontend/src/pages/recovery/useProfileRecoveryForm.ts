import { useState, type FormEvent } from 'react'
import type { ProfileRecoveryResponse } from '@/api/generated/types.gen'
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { ensureSetupRealtimeSync, refreshSetupState } from '@/store/setupRealtimeStore'

const log = createLogger('profile-recovery-page')

/**
 * Passphrase recovery of a profile whose saved system key is missing, shared by every skin. The
 * daemon's recovery status decides whether a passphrase can be submitted or a restart is needed.
 */
export function useProfileRecoveryForm(status: ProfileRecoveryResponse, onRecovered: () => void) {
  const [form, setForm] = useState({ passphrase: '', submitting: false, errorKey: '' })

  const busy = form.submitting || status.state === 'recovering'
  const statusErrorKey =
    status.state === 'partially_recoverable'
      ? 'profileRecovery.partial'
      : status.state === 'failed'
        ? 'profileRecovery.failed'
        : ''
  const errorKey = status.restartRequired
    ? 'profileRecovery.restartRequired'
    : form.errorKey || statusErrorKey
  const inputDisabled = busy || status.restartRequired || !status.canSubmitPassphrase

  const submit = async (event?: FormEvent) => {
    event?.preventDefault()
    if (busy || status.restartRequired || !status.canSubmitPassphrase || !form.passphrase) return
    setForm(value => ({ ...value, submitting: true, errorKey: '' }))
    try {
      await commands.unlockContent({ passphrase: form.passphrase })
      setForm({ passphrase: '', submitting: false, errorKey: '' })
      try {
        await ensureSetupRealtimeSync()
        await refreshSetupState()
      } catch {
        log.warn('Setup refresh pending after profile recovery')
      }
      onRecovered()
    } catch (error) {
      setForm(value => ({ ...value, submitting: false, errorKey: contentUnlockErrorKey(error) }))
    }
  }

  return {
    passphrase: form.passphrase,
    setPassphrase: (passphrase: string) =>
      setForm(value => ({ ...value, passphrase, errorKey: '' })),
    submitting: form.submitting,
    /** An error from the last submission, as opposed to one reported by the recovery status. */
    submitErrorKey: form.errorKey,
    busy,
    errorKey,
    /** Some original keys are missing; shown while no more specific error is. */
    showLosses: status.losses.length > 0 && !errorKey,
    requiresSupport:
      status.state === 'partially_recoverable' &&
      !status.canSubmitPassphrase &&
      !status.restartRequired &&
      !busy,
    inputDisabled,
    canSubmit: !busy && status.canSubmitPassphrase && form.passphrase.length > 0,
    restartRequired: status.restartRequired,
    submit,
  }
}
