import { useCallback, useEffect, useEffectEvent, useMemo, useReducer } from 'react'
import { useTranslation } from 'react-i18next'
import {
  SetupV2Error,
  cancelJoinSpace,
  switchSpace,
  type SwitchSpaceErrorKind,
  type SwitchSpaceResponse,
} from '@/api/daemon/setupV2'
import { type JoinAdmissionResolution, useJoinAdmission } from '@/hooks/useJoinAdmission'
import { INVITATION_CODE_LENGTH } from '@/lib/invitation-code'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { fetchLocalDeviceInfo, fetchSpaceMembers } from '@/store/slices/devicesSlice'

const log = createLogger('switch-space-dialog')

const SUCCESS_AUTO_CLOSE_MS = 2500

type Step = 'input' | 'migrating' | 'pending' | 'success' | 'failed'
type ActiveJoinSpaceResponse = Extract<SwitchSpaceResponse, { status: 'active' }>

interface SwitchState {
  step: Step
  code: string
  pass: string
  showPass: boolean
  errorKind: SwitchSpaceErrorKind | null
  errorRaw: string | null
  result: ActiveJoinSpaceResponse | null
  pendingJoinId: string | null
}

export function useSwitchSpace({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation(undefined, { keyPrefix: 'devices.switchSpace' })
  const dispatch = useAppDispatch()
  const [state, update] = useReducer(
    (current: SwitchState, changes: Partial<SwitchState>) => ({ ...current, ...changes }),
    {
      step: 'input',
      code: '',
      pass: '',
      showPass: false,
      errorKind: null,
      errorRaw: null,
      result: null,
      pendingJoinId: null,
    }
  )
  const { step, code, pass, errorKind, errorRaw, pendingJoinId } = state

  const codeComplete = code.length === INVITATION_CODE_LENGTH
  const canSubmit = codeComplete && pass.length > 0 && step === 'input'

  // Keep the close timer stable across parent renders.
  const closeDialog = useEffectEvent(() => onOpenChange(false))
  useEffect(() => {
    if (step !== 'success') return
    const id = setTimeout(() => closeDialog(), SUCCESS_AUTO_CLOSE_MS)
    return () => clearTimeout(id)
  }, [step])

  const resolveJoinAdmission = useCallback(
    (resolution: JoinAdmissionResolution) => {
      if (resolution.status === 'active') {
        dispatch(fetchSpaceMembers())
        dispatch(fetchLocalDeviceInfo())
        update({ result: resolution, pendingJoinId: null, step: 'success' })
        return
      }
      update({
        pendingJoinId: null,
        errorKind: 'internal',
        errorRaw: resolution.reason,
        step: 'failed',
      })
    },
    [dispatch]
  )
  useJoinAdmission(pendingJoinId, resolveJoinAdmission)

  const handleSubmit = async (preserveUnreadableHistory = false) => {
    // Validate inputs independently from step check
    if (!codeComplete || pass.length === 0) return
    // Allow submission when in 'input' step OR when preserveUnreadableHistory retry is triggered
    if (step !== 'input' && !preserveUnreadableHistory) return
    update({ errorKind: null, errorRaw: null, step: 'migrating' })
    try {
      const res = await switchSpace({
        code,
        newPassphrase: pass,
        preserveUnreadableHistory,
      })
      if (res.status === 'active') {
        resolveJoinAdmission(res)
      } else if (res.status === 'pending') {
        update({ pendingJoinId: res.joinId, step: 'pending' })
      } else if (res.status === 'rejected') {
        update({ errorKind: 'internal', errorRaw: res.reason, step: 'failed' })
      }
    } catch (err) {
      log.error({ err }, 'switchSpace failed')
      if (err instanceof SetupV2Error) {
        update({ errorKind: err.kind as SwitchSpaceErrorKind, errorRaw: err.raw })
      } else {
        update({
          errorKind: 'internal',
          errorRaw: err instanceof Error ? err.message : String(err),
        })
      }
      update({ step: 'failed' })
    }
  }

  // Invalidated invitations require a new code when retrying.
  const isCodeDead =
    errorKind === 'invitation_not_found' ||
    errorKind === 'invitation_expired' ||
    errorKind === 'sponsor_rejected'

  const handleRetry = () => {
    if (isCodeDead) {
      update({ code: '', pass: '' })
    } else if (errorKind === 'passphrase_mismatch') {
      update({ pass: '' })
    }
    update({ errorKind: null, errorRaw: null, step: 'input' })
  }

  const handleCancelPending = async () => {
    if (!pendingJoinId) return
    try {
      const result = await cancelJoinSpace(pendingJoinId)
      if (result.status === 'active') {
        resolveJoinAdmission(result)
      } else if (result.status === 'rejected') {
        resolveJoinAdmission(result)
      }
    } catch (err) {
      log.error({ err, joinId: pendingJoinId }, 'cancelJoinSpace failed')
      update({
        errorKind: 'internal',
        errorRaw: err instanceof Error ? err.message : String(err),
        step: 'failed',
      })
    }
  }

  const retryLabel = isCodeDead ? t('actions.useNewCode') : t('actions.retry')

  const failureMessage = useMemo(() => {
    if (!errorKind) return null
    const key = `failed.reasons.${errorKind}`
    const translated = t(key)
    if (translated !== key) return translated
    return t('failed.fallback', { reason: errorRaw ?? errorKind })
  }, [errorKind, errorRaw, t])

  return {
    ...state,
    codeComplete,
    canSubmit,
    retryLabel,
    failureMessage,
    setCode: (code: string) => update({ code }),
    setPass: (pass: string) => update({ pass }),
    togglePassVisibility: () => update({ showPass: !state.showPass }),
    handleSubmit,
    handleRetry,
    handleCancelPending,
  }
}
