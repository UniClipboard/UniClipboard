import { useState, type FormEvent } from 'react'
import { contentUnlockErrorKey } from '@/lib/content-unlock-error'
import { commands } from '@/lib/ipc'

/** Passphrase submission of the unlock page; Engine validates the passphrase. */
export function usePassphraseUnlock(
  onUnlocked: () => Promise<void>,
  onSubmittingChange: (submitting: boolean) => void
) {
  const [passphrase, setPassphraseValue] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [errorKey, setErrorKey] = useState<string | null>(null)

  const setBusy = (busy: boolean) => {
    setSubmitting(busy)
    onSubmittingChange(busy)
  }

  const submit = async (event?: FormEvent) => {
    event?.preventDefault()
    if (submitting) return
    if (passphrase.length === 0) {
      setErrorKey('unlock.errors.wrongPassphrase')
      return
    }

    setBusy(true)
    setErrorKey(null)
    try {
      await commands.unlockContent({ passphrase })
      setPassphraseValue('')
      await onUnlocked()
    } catch (error) {
      setErrorKey(contentUnlockErrorKey(error))
    } finally {
      setBusy(false)
    }
  }

  const setPassphrase = (value: string) => {
    setPassphraseValue(value)
    setErrorKey(null)
  }

  return { passphrase, setPassphrase, submitting, errorKey, submit }
}
