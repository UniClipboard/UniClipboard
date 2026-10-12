import { useEffect, useReducer, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { getSettings } from '@/api/daemon/settings'
import type { InitializeSpaceErrorKind } from '@/api/daemon/setupV2'
import { initializeErrorMessage } from './setup-error-messages'

export type InitializeSpaceSubmit = (input: {
  deviceName: string
  passphrase: string
  passphraseConfirm: string
}) => Promise<{ ok: true } | { ok: false; kind: InitializeSpaceErrorKind; raw: string }>

interface InitializeForm {
  deviceName: string
  pass1: string
  pass2: string
  errorKind: InitializeSpaceErrorKind | null
}

type InitializeFormAction =
  | { type: 'default_name'; name: string }
  | { type: 'edit'; changes: Partial<InitializeForm> }

/**
 * Form state, validation and submission of the "create your space" step, shared by every skin of
 * the setup page. Validation errors belong to the field they describe; service errors are general.
 */
export function useInitializeSpaceForm(onSubmit: InitializeSpaceSubmit, loading?: boolean) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.initializeSpace' })
  const [form, updateForm] = useReducer(
    (state: InitializeForm, action: InitializeFormAction): InitializeForm =>
      action.type === 'default_name'
        ? { ...state, deviceName: state.deviceName || action.name }
        : { ...state, ...action.changes },
    { deviceName: '', pass1: '', pass2: '', errorKind: null }
  )
  const { deviceName, pass1, pass2, errorKind } = form

  const errorMessage = initializeErrorMessage(t, errorKind)
  const deviceNameError = errorKind === 'device_name_required' ? errorMessage : null
  const passphraseError = errorKind === 'passphrase_mismatch' ? errorMessage : null
  const generalError = deviceNameError || passphraseError ? null : errorMessage

  // Pre-fill the device name with the daemon-resolved default (the OS
  // hostname, written during bootstrap). Skip the write if the user has
  // already typed something so a late response never clobbers their input.
  useEffect(() => {
    let cancelled = false
    getSettings()
      .then(s => {
        if (cancelled) return
        const fallback = s.general.deviceName?.trim() ?? ''
        if (!fallback) return
        updateForm({ type: 'default_name', name: fallback })
      })
      .catch(() => {
        // Non-fatal — user can still type a name manually.
      })
    return () => {
      cancelled = true
    }
  }, [])

  const submit = async (event?: FormEvent) => {
    event?.preventDefault()
    if (loading) return
    updateForm({ type: 'edit', changes: { errorKind: null } })
    if (!deviceName.trim()) {
      updateForm({ type: 'edit', changes: { errorKind: 'device_name_required' } })
      return
    }
    if (!pass1 || pass1 !== pass2) {
      updateForm({ type: 'edit', changes: { errorKind: 'passphrase_mismatch' } })
      return
    }
    const res = await onSubmit({
      deviceName: deviceName.trim(),
      passphrase: pass1,
      passphraseConfirm: pass2,
    })
    if (!res.ok) updateForm({ type: 'edit', changes: { errorKind: res.kind } })
  }

  return {
    deviceName,
    pass1,
    pass2,
    setDeviceName: (value: string) => updateForm({ type: 'edit', changes: { deviceName: value } }),
    setPass1: (value: string) => updateForm({ type: 'edit', changes: { pass1: value } }),
    setPass2: (value: string) => updateForm({ type: 'edit', changes: { pass2: value } }),
    deviceNameError,
    passphraseError,
    generalError,
    submit,
  }
}
