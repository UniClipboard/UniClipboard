import { useEffect, useEffectEvent, useMemo, useReducer, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getDeviceTrustSnapshot } from '@/api/daemon/device-trust'
import {
  cancelInvitation,
  getSetupState,
  issuePairingInvitation,
  type CurrentInvitation,
} from '@/api/daemon/setupV2'
import { isUnlockSpaceError, unlockSpaceWithPassphrase } from '@/api/security'
import type { SetupInvitationRevokedEvent } from '@/api/setupEvents'
import { activeDeviceIds, findNewActiveDeviceId } from '@/components/device/pairing-success-utils'
import { daemonWs } from '@/lib/daemon-ws'
import { formatInvitationCode } from '@/lib/invitation-code'
import { createLogger } from '@/lib/logger'

const log = createLogger('add-device-dialog')

// Estimate progress for restored invitations using the default lifetime.
const DEFAULT_TTL_MS = 5 * 60 * 1000
// Keep the success message visible before closing.
const SUCCESS_AUTO_CLOSE_MS = 5000

type Step = 'credentials' | 'invitation' | 'success' | 'failed'
interface InvitationState {
  invitation: CurrentInvitation | null
  issuedAtMs: number | null
  loading: boolean
  error: string | null
  step: Step
  failureReason: string | null
  passphrase: string
}

type PairingCompletionTrigger = 'device_trust_changed' | 'refresh_required' | 'reconnected'

export function useAddDeviceInvitation({
  open,
  onOpenChange,
  onSuccess,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSuccess?: () => void
}) {
  const { t } = useTranslation()
  const [state, update] = useReducer(
    (current: InvitationState, changes: Partial<InvitationState>) => ({ ...current, ...changes }),
    {
      invitation: null,
      issuedAtMs: null,
      loading: false,
      error: null,
      step: 'invitation',
      failureReason: null,
      passphrase: '',
    }
  )
  const { invitation, issuedAtMs, loading, step, failureReason, passphrase } = state
  const [now, setNow] = useState(() => Date.now())
  const [copied, setCopied] = useState(false)
  const initialDeviceIdsRef = useRef<ReadonlySet<string> | null>(null)
  const pairingCompletionInFlightRef = useRef(false)
  const reportSuccess = useEffectEvent(() => onSuccess?.())

  // Tick only while displaying a live invitation.
  useEffect(() => {
    if (!open || !invitation || step !== 'invitation') return
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [open, invitation, step])

  // Restore the current invitation when opening, or issue one if necessary.
  // Closing does not revoke it; local form state resets after the exit finishes.
  // Keep translation updates out of the effect dependencies so resource reloads
  // cannot issue another invitation or overwrite the completed state.
  const reportIssueFailure = useEffectEvent(() => {
    log.error({ error_kind: 'invitation_issue_failed' }, 'failed to load or issue invitation')
    update({ error: t('devices.addDevice.errors.issueFailed') })
  })
  useEffect(() => {
    if (!open) return
    let cancelled = false
    void (async () => {
      update({ loading: true, error: null })
      try {
        initialDeviceIdsRef.current = activeDeviceIds(await getDeviceTrustSnapshot())
        const state = await getSetupState()
        if (cancelled) return
        if (state.currentInvitation) {
          update({ invitation: state.currentInvitation })
          // Estimate the issue time for a restored invitation.
          update({ issuedAtMs: state.currentInvitation.expiresAtMs - DEFAULT_TTL_MS })
          log.info({ event: 'invitation_ready', mode: 'reused' }, 'pairing invitation ready')
        } else if (state.rePairingRequired) {
          update({ step: 'credentials' })
          log.info({ event: 'credentials_required' }, 're-pairing credentials required')
        } else {
          const issued = await issuePairingInvitation()
          if (cancelled) return
          update({ invitation: issued, issuedAtMs: Date.now() })
          log.info({ event: 'invitation_ready', mode: 'standard' }, 'pairing invitation ready')
        }
      } catch {
        if (cancelled) return
        reportIssueFailure()
      } finally {
        if (!cancelled) update({ loading: false })
      }
    })()
    return () => {
      cancelled = true
    }
  }, [open])

  const confirmPairingCompleted = useEffectEvent(async (trigger: PairingCompletionTrigger) => {
    if (
      step !== 'invitation' ||
      initialDeviceIdsRef.current === null ||
      pairingCompletionInFlightRef.current
    ) {
      return
    }
    try {
      const trust = await getDeviceTrustSnapshot()
      const peerDeviceId = findNewActiveDeviceId(
        initialDeviceIdsRef.current,
        activeDeviceIds(trust)
      )
      if (peerDeviceId === null) return
      pairingCompletionInFlightRef.current = true
      try {
        await cancelInvitation()
      } catch {
        log.warn(
          { error_kind: 'invitation_cleanup_failed' },
          'failed to clear completed invitation'
        )
      }
      update({ step: 'success' })
      log.info({ event: 'pairing_confirmed', trigger }, 'new device pairing confirmed')
      reportSuccess()
    } catch {
      log.warn(
        { error_kind: 'pairing_confirmation_failed', trigger },
        'failed to verify completed invitation'
      )
      pairingCompletionInFlightRef.current = false
    }
  })

  const handleInvitationRevoked = useEffectEvent((evt: SetupInvitationRevokedEvent) => {
    if (step !== 'invitation' || loading) return
    update({ failureReason: evt.reason, step: 'failed' })
    log.info({ event: 'invitation_revoked' }, 'pairing invitation revoked')
  })

  useEffect(() => {
    if (!open) return
    const unsubscribeEvents = daemonWs.subscribe(['device-trust', 'setup', 'system'], event => {
      if (event.eventType === 'device-trust.changed') {
        void confirmPairingCompleted('device_trust_changed')
      } else if (event.eventType === 'system.refresh_required') {
        void confirmPairingCompleted('refresh_required')
      } else if (event.eventType === 'setup.invitationRevoked') {
        handleInvitationRevoked(event.payload as SetupInvitationRevokedEvent)
      }
    })
    const unsubscribeReconnect = daemonWs.onReconnect(
      () => void confirmPairingCompleted('reconnected')
    )

    return () => {
      unsubscribeEvents()
      unsubscribeReconnect()
    }
  }, [open])

  // Keep the close timer stable across parent renders.
  const closeDialog = useEffectEvent(() => onOpenChange(false))
  useEffect(() => {
    if (step !== 'success') return
    const id = setTimeout(() => closeDialog(), SUCCESS_AUTO_CLOSE_MS)
    return () => clearTimeout(id)
  }, [step])

  const remaining = invitation ? Math.max(0, invitation.expiresAtMs - now) : 0
  const expired = invitation && step === 'invitation' ? remaining <= 0 : false
  const totalMs = invitation && issuedAtMs ? invitation.expiresAtMs - issuedAtMs : DEFAULT_TTL_MS
  const progress = invitation ? Math.max(0, Math.min(100, (remaining / totalMs) * 100)) : 0
  const display = useMemo(
    () => (invitation ? formatInvitationCode(invitation.code) : ''),
    [invitation]
  )

  const handleCopy = async () => {
    if (!invitation) return
    try {
      await navigator.clipboard.writeText(invitation.code)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch {
      log.warn({ error_kind: 'clipboard_write_failed' }, 'clipboard.writeText failed')
    }
  }

  const handleCancel = async () => {
    update({ loading: true })
    try {
      await cancelInvitation()
    } catch {
      log.warn({ error_kind: 'invitation_cancel_failed' }, 'cancelInvitation failed on close')
    } finally {
      update({ loading: false })
      onOpenChange(false)
    }
  }

  const handleRegenerate = async () => {
    update({ loading: true, error: null, step: 'invitation', failureReason: null })
    try {
      initialDeviceIdsRef.current = activeDeviceIds(await getDeviceTrustSnapshot())
      try {
        await cancelInvitation()
      } catch {
        log.warn(
          { error_kind: 'invitation_cancel_failed' },
          'cancelInvitation before regenerate failed'
        )
      }
      const issued = await issuePairingInvitation()
      update({ invitation: issued, issuedAtMs: Date.now() })
      log.info({ event: 'invitation_ready', mode: 'regenerated' }, 'pairing invitation ready')
    } catch {
      log.error({ error_kind: 'invitation_issue_failed' }, 'regenerate invitation failed')
      update({ error: t('devices.addDevice.errors.issueFailed') })
    } finally {
      update({ loading: false })
    }
  }

  const handleConfirmPassphrase = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const value = passphrase.trim()
    if (!value) return
    update({ loading: true, error: null })
    log.info({ event: 'credentials_submitted' }, 're-pairing credentials submitted')
    try {
      await unlockSpaceWithPassphrase(value)
      initialDeviceIdsRef.current = activeDeviceIds(await getDeviceTrustSnapshot())
      const issued = await issuePairingInvitation()
      update({ invitation: issued, issuedAtMs: Date.now(), passphrase: '', step: 'invitation' })
      log.info(
        { event: 'invitation_ready', mode: 'legacy_re_pairing' },
        're-pairing invitation ready'
      )
    } catch (err) {
      const wrongPassphrase = isUnlockSpaceError(err) && err.code === 'WRONG_PASSPHRASE'
      if (wrongPassphrase) {
        log.info(
          { error_kind: 'wrong_passphrase', event: 'credentials_rejected' },
          're-pairing credentials rejected'
        )
      } else {
        log.warn(
          { error_kind: 'credentials_confirmation_failed', event: 'credentials_rejected' },
          're-pairing credentials rejected'
        )
      }
      update({
        error: wrongPassphrase
          ? t('devices.addDevice.rePairing.wrongPassphrase')
          : t('devices.addDevice.rePairing.failed'),
      })
    } finally {
      update({ loading: false })
    }
  }

  const failureMessage = useMemo(() => {
    if (!failureReason) return t('devices.addDevice.failed.unknown')
    const key = `devices.addDevice.failed.reasons.${failureReason}`
    const translated = t(key)
    if (translated !== key) return translated
    return t('devices.addDevice.failed.fallback', { reason: failureReason })
  }, [failureReason, t])

  return {
    ...state,
    copied,
    remaining,
    expired,
    progress,
    display,
    failureMessage,
    setPassphrase: (passphrase: string) => update({ passphrase }),
    handleCopy,
    handleCancel,
    handleRegenerate,
    handleConfirmPassphrase,
  }
}
