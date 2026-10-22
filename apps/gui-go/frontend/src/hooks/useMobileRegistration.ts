import { useCallback, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import {
  isMobileSyncError,
  registerMobileDevice,
  type MobileSyncError,
  type RegisterMobileDeviceResult,
} from '@/api/tauri-command/mobile_sync'
import { createLogger } from '@/lib/logger'

const log = createLogger('mobile-registration')

type FieldErrorKey = 'label' | 'username' | 'password'
type FieldErrors = Partial<Record<FieldErrorKey, string>>

interface FormState {
  label: string
  username: string
  password: string
  advancedOpen: boolean
  submitting: boolean
  fieldErrors: FieldErrors
  formError: string | null
}

type FormAction =
  | { type: 'edit'; field: FieldErrorKey; value: string }
  | { type: 'advanced'; open: boolean }
  | { type: 'submit' }
  | { type: 'fieldError'; field: FieldErrorKey; message: string }
  | { type: 'formError'; message: string }
  | { type: 'settled' }

function formReducer(state: FormState, action: FormAction): FormState {
  switch (action.type) {
    case 'edit': {
      const fieldErrors = { ...state.fieldErrors }
      delete fieldErrors[action.field]
      return { ...state, [action.field]: action.value, fieldErrors }
    }
    case 'advanced':
      return { ...state, advancedOpen: action.open }
    case 'submit':
      return { ...state, submitting: true, fieldErrors: {}, formError: null }
    case 'fieldError':
      return {
        ...state,
        fieldErrors: { [action.field]: action.message },
        formError: null,
        advancedOpen: state.advancedOpen || action.field !== 'label',
      }
    case 'formError':
      return { ...state, formError: action.message }
    case 'settled':
      return { ...state, submitting: false }
  }
}

export function useMobileRegistration({
  onBusyChange,
  onSuccess,
}: {
  onBusyChange: (busy: boolean) => void
  onSuccess: (result: RegisterMobileDeviceResult) => void
}) {
  const { t } = useTranslation()
  const [
    { label, username, password, advancedOpen, submitting, fieldErrors, formError },
    dispatch,
  ] = useReducer(formReducer, {
    label: '',
    username: '',
    password: '',
    advancedOpen: false,
    submitting: false,
    fieldErrors: {},
    formError: null,
  })

  const handleSubmit = useCallback(async () => {
    const trimmedLabel = label.trim()
    if (trimmedLabel === '') {
      dispatch({
        type: 'fieldError',
        field: 'label',
        message: t('devices.mobileSync.errors.labelEmpty'),
      })
      return
    }
    dispatch({ type: 'submit' })
    onBusyChange(true)
    try {
      const result = await registerMobileDevice({
        label: trimmedLabel,
        username: username.trim() || undefined,
        password: password || undefined,
      })
      onSuccess(result)
    } catch (err) {
      log.error({ err }, 'failed to register mobile device')
      const error = classifyRegisterError(t, err)
      if (error.kind === 'field') {
        dispatch({
          type: 'fieldError',
          field: error.field,
          message: error.message,
        })
      } else {
        dispatch({ type: 'formError', message: error.message })
      }
    } finally {
      dispatch({ type: 'settled' })
      onBusyChange(false)
    }
  }, [label, onBusyChange, onSuccess, password, t, username])

  return {
    label,
    username,
    password,
    advancedOpen,
    submitting,
    fieldErrors,
    formError,
    dispatch,
    handleSubmit,
  }
}

type RegisterErrorDispatch =
  | { kind: 'field'; field: FieldErrorKey; message: string }
  | { kind: 'form'; message: string }

// Map typed service failures to a field or the form.
function classifyRegisterError(
  t: ReturnType<typeof useTranslation>['t'],
  err: unknown
): RegisterErrorDispatch {
  if (isMobileSyncError(err)) {
    const e = err as MobileSyncError
    switch (e.code) {
      case 'LABEL_EMPTY':
        return {
          kind: 'field',
          field: 'label',
          message: t('devices.mobileSync.errors.labelEmpty'),
        }
      case 'LABEL_TOO_LONG':
        return {
          kind: 'field',
          field: 'label',
          message: t('devices.mobileSync.errors.labelTooLong', { max: e.max }),
        }
      case 'USERNAME_TAKEN':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTaken', {
            username: e.username,
          }),
        }
      case 'USERNAME_TOO_SHORT':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTooShort', {
            min: e.min,
            got: e.got,
          }),
        }
      case 'USERNAME_TOO_LONG':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameTooLong', {
            max: e.max,
            got: e.got,
          }),
        }
      case 'USERNAME_MUST_START_WITH_LETTER':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameMustStartWithLetter'),
        }
      case 'USERNAME_CONTAINS_FORBIDDEN_CHARS':
        return {
          kind: 'field',
          field: 'username',
          message: t('devices.mobileSync.errors.usernameContainsForbiddenChars'),
        }
      case 'PASSWORD_TOO_SHORT':
        return {
          kind: 'field',
          field: 'password',
          message: t('devices.mobileSync.errors.passwordTooShort', {
            min: e.min,
          }),
        }
      case 'PASSWORD_TOO_LONG':
        return {
          kind: 'field',
          field: 'password',
          message: t('devices.mobileSync.errors.passwordTooLong', {
            max: e.max,
          }),
        }
      case 'LAN_LISTENER_DISABLED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.lanListenerDisabled'),
        }
      case 'PASSWORD_HASH_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.passwordHashFailed', {
            message: e.message,
          }),
        }
      case 'PERSISTENCE_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.persistenceFailed', {
            message: e.message,
          }),
        }
      case 'QR_RENDER_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.qrRenderFailed', {
            message: e.message,
          }),
        }
      case 'SETTINGS_LOAD_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.settingsLoadFailed', {
            message: e.message,
          }),
        }
      case 'FACADE_UNAVAILABLE':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.facadeUnavailable'),
        }
      case 'NO_LAN_INTERFACE_AVAILABLE':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.noLanInterfaceAvailable'),
        }
      case 'LAN_PROBE_FAILED':
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.lanProbeFailed', {
            message: e.message,
          }),
        }
      default: {
        // Keep unexpected service failures visible in the form.
        const message = (e as { message?: string }).message ?? e.code
        return {
          kind: 'form',
          message: t('devices.mobileSync.errors.unknown', { message }),
        }
      }
    }
  }
  const message = err instanceof Error ? err.message : String(err)
  return {
    kind: 'form',
    message: t('devices.mobileSync.errors.unknown', { message }),
  }
}
