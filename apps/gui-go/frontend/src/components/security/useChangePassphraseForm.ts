import { useEffect, useReducer, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { changeEncryptionPassphrase, getChangePassphraseErrorCode } from '@/api/daemon/encryption'
import { toast } from '@/components/ui/toast'
import { createLogger } from '@/lib/logger'

const log = createLogger('change-passphrase')

interface ChangePassphraseState {
  passphrase: string
  confirmation: string
  visible: boolean
  errorKey: string | null
}

type ChangePassphraseAction =
  | { type: 'edit'; field: 'passphrase' | 'confirmation'; value: string }
  | { type: 'toggle_visibility' }
  | { type: 'failed'; errorKey: string }
  | { type: 'reset' }

const initialState: ChangePassphraseState = {
  passphrase: '',
  confirmation: '',
  visible: false,
  errorKey: null,
}

function reducer(
  state: ChangePassphraseState,
  action: ChangePassphraseAction
): ChangePassphraseState {
  switch (action.type) {
    case 'reset':
      return initialState
    case 'edit':
      return { ...state, [action.field]: action.value, errorKey: null }
    case 'toggle_visibility':
      return { ...state, visible: !state.visible }
    case 'failed':
      return { ...state, errorKey: action.errorKey }
  }
}

function errorTranslationKey(error: unknown): string {
  switch (getChangePassphraseErrorCode(error)) {
    case 'PASSPHRASE_MISMATCH':
      return 'passphraseChange.errors.mismatch'
    case 'MULTIPLE_DEVICES':
      return 'passphraseChange.errors.multipleDevices'
    case 'SPACE_LOCKED':
      return 'passphraseChange.errors.locked'
    case 'MEMBERSHIP_RECOVERY_REQUIRED':
    case 'RECOVERY_REQUIRED':
      return 'passphraseChange.errors.recovery'
    case 'runtime_unavailable':
      return 'passphraseChange.errors.unavailable'
    default:
      return 'passphraseChange.errors.internal'
  }
}

/** Owns the draft, request and outcome independently of the form view. */
export function useChangePassphraseForm({
  submitting,
  onSubmittingChange,
  onChanged,
}: {
  submitting: boolean
  onSubmittingChange: (submitting: boolean) => void
  onChanged: () => void
}) {
  const { t } = useTranslation()
  const [state, dispatch] = useReducer(reducer, initialState)
  const { passphrase, confirmation } = state
  const mismatch = confirmation.length > 0 && passphrase !== confirmation

  const mounted = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!passphrase || !confirmation || submitting) return
    if (passphrase !== confirmation) {
      dispatch({ type: 'failed', errorKey: 'passphraseChange.errors.mismatch' })
      return
    }

    onSubmittingChange(true)
    try {
      await changeEncryptionPassphrase(passphrase, confirmation)
      log.info({ event: 'passphrase_change_succeeded' }, 'space passphrase changed')
      if (mounted.current) {
        dispatch({ type: 'reset' })
        onChanged()
      } else toast.success(t('passphraseChange.success'))
    } catch (error) {
      log.warn(
        { error_kind: getChangePassphraseErrorCode(error) ?? 'unknown' },
        'space passphrase change failed'
      )
      const errorKey = errorTranslationKey(error)
      if (mounted.current) dispatch({ type: 'failed', errorKey })
      else toast.error(t(errorKey))
    } finally {
      if (mounted.current) onSubmittingChange(false)
    }
  }

  return {
    state,
    dispatch,
    submitting,
    mismatch,
    handleSubmit,
    reset: () => dispatch({ type: 'reset' }),
  }
}
