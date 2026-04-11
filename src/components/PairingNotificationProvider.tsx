import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { classifyPairingError } from '@/api/daemon/events'
import { acceptP2PPairing, rejectP2PPairing } from '@/api/daemon/pairing'
import PairingPinDialog from '@/components/PairingPinDialog'
import { usePairingEvents } from '@/hooks/useDaemonEvents'
import { createLogger } from '@/lib/logger'

const log = createLogger('pairing-notification-provider')

// ── Provider-level pairing diagnostics ─────────────────────────────────────

/**
 * Record a session-aware provider decision for pairing lifecycle events.
 *
 * Security: never logs code, fingerprint, or passphrase fields.
 */
function logProviderDecision(
  decision: 'accepted' | 'rejected' | 'ignored' | 'canceled' | 'success' | 'failure',
  context: {
    path: string
    sessionId?: string | null
    activeSessionId?: string | null
    reason?: string
  }
) {
  const { path, sessionId, activeSessionId, reason } = context
  const parts: string[] = [`[PairingNotificationProvider] ${decision}`, `path=${path}`]
  if (sessionId) parts.push(`session_id=${sessionId}`)
  if (activeSessionId !== undefined) parts.push(`active_session_id=${activeSessionId ?? 'null'}`)
  if (reason) parts.push(`reason=${reason}`)

  log.debug(parts.join(' '))
}

export function PairingNotificationProvider() {
  const { t } = useTranslation()
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null)
  const activeSessionIdRef = useRef(activeSessionId)

  const [dialogState, setDialogState] = useState<{
    open: boolean
    pinCode: string
    peerDeviceName?: string
    peerId?: string
    phase?: 'display' | 'verifying' | 'success'
  }>({
    open: false,
    pinCode: '',
  })

  useEffect(() => {
    activeSessionIdRef.current = activeSessionId
  }, [activeSessionId])

  const localizePairingError = useCallback(
    (error?: string | null) => {
      switch (classifyPairingError(error)) {
        case 'active_session_exists':
          return t('pairing.failed.errors.activeSession')
        case 'no_local_participant':
          return t('pairing.failed.errors.noParticipant')
        case 'session_not_found':
          return t('pairing.failed.errors.sessionExpired')
        case 'daemon_unavailable':
          return t('pairing.failed.errors.daemonUnavailable')
        default:
          return error || t('pairing.failed.title', { defaultValue: 'Pairing failed' })
      }
    },
    [t]
  )

  usePairingEvents({
    onRequest: ({ sessionId, peerId, deviceName }) => {
      toast(
        t('pairing.request.title', {
          defaultValue: 'Pairing Request',
          device: deviceName || 'Unknown Device',
        }),
        {
          description: t('pairing.request.description', {
            defaultValue: 'A device wants to pair with you',
          }),
          action: {
            label: t('common.accept', { defaultValue: 'Accept' }),
            onClick: () => {
              logProviderDecision('accepted', { path: 'request', sessionId })
              activeSessionIdRef.current = sessionId
              setActiveSessionId(sessionId)
              acceptP2PPairing(sessionId).catch(err => {
                const message = localizePairingError(
                  err instanceof Error ? err.message : String(err)
                )
                logProviderDecision('failure', {
                  path: 'request.accept_api',
                  sessionId,
                  reason: 'acceptP2PPairing_rejected',
                })
                log.error({ err }, 'Failed to accept pairing request')
                toast.error(t('pairing.failed.title', { defaultValue: 'Pairing failed' }), {
                  description: message,
                })
                activeSessionIdRef.current = null
                setActiveSessionId(null)
              })
            },
          },
          cancel: {
            label: t('common.reject', { defaultValue: 'Reject' }),
            onClick: () => {
              logProviderDecision('rejected', { path: 'request', sessionId })
              if (peerId) {
                rejectP2PPairing(sessionId, peerId).catch(err => {
                  const message = localizePairingError(
                    err instanceof Error ? err.message : String(err)
                  )
                  logProviderDecision('failure', {
                    path: 'request.reject_api',
                    sessionId,
                    reason: 'rejectP2PPairing_rejected',
                  })
                  log.error({ err }, 'Failed to reject pairing request')
                  toast.error(t('pairing.failed.title', { defaultValue: 'Pairing failed' }), {
                    description: message,
                  })
                })
              }
            },
          },
          duration: 30_000,
        }
      )
    },

    onVerification: ({ sessionId, deviceName, code, peerId }) => {
      const currentSessionId = activeSessionIdRef.current
      if (!currentSessionId || sessionId !== currentSessionId) {
        logProviderDecision('ignored', {
          path: 'verification',
          sessionId,
          activeSessionId: currentSessionId,
          reason: currentSessionId ? 'session_mismatch' : 'no_active_session',
        })
        return
      }

      logProviderDecision('accepted', { path: 'verification', sessionId })
      setDialogState({
        open: true,
        pinCode: code ?? '',
        peerDeviceName: deviceName,
        peerId: peerId,
        phase: 'display',
      })
    },

    onVerifying: ({ sessionId }) => {
      const currentSessionId = activeSessionIdRef.current
      if (!currentSessionId || sessionId !== currentSessionId) {
        logProviderDecision('ignored', {
          path: 'verifying',
          sessionId,
          activeSessionId: currentSessionId,
          reason: currentSessionId ? 'session_mismatch' : 'no_active_session',
        })
        return
      }
      logProviderDecision('accepted', { path: 'verifying', sessionId })
      setDialogState(prev => ({ ...prev, phase: 'verifying' }))
    },

    onComplete: ({ sessionId }) => {
      const currentSessionId = activeSessionIdRef.current
      if (!currentSessionId || sessionId !== currentSessionId) {
        logProviderDecision('ignored', {
          path: 'complete',
          sessionId,
          activeSessionId: currentSessionId,
          reason: currentSessionId ? 'session_mismatch' : 'no_active_session',
        })
        return
      }
      logProviderDecision('success', { path: 'complete', sessionId })
      setDialogState(prev => ({ ...prev, phase: 'verifying' }))
    },

    onFailed: ({ sessionId, error }) => {
      const currentSessionId = activeSessionIdRef.current
      if (!currentSessionId || sessionId !== currentSessionId) {
        logProviderDecision('ignored', {
          path: 'failed',
          sessionId,
          activeSessionId: currentSessionId,
          reason: currentSessionId ? 'session_mismatch' : 'no_active_session',
        })
        return
      }
      logProviderDecision('failure', { path: 'failed', sessionId })
      setDialogState(prev => ({ ...prev, open: false }))
      toast.error(t('pairing.failed.title', { defaultValue: 'Pairing failed' }), {
        description: localizePairingError(error),
      })
      setActiveSessionId(null)
    },

    onSpaceAccessCompleted: ({ sessionId, success, reason }) => {
      const currentSessionId = activeSessionIdRef.current
      if (!currentSessionId || sessionId !== currentSessionId) {
        logProviderDecision('ignored', {
          path: 'spaceAccessCompleted',
          sessionId,
          activeSessionId: currentSessionId,
          reason: currentSessionId ? 'session_mismatch' : 'no_active_session',
        })
        return
      }

      if (success) {
        logProviderDecision('success', { path: 'spaceAccessCompleted', sessionId })
        setDialogState(prev => ({ ...prev, phase: 'success' }))
        setTimeout(() => {
          setDialogState(prev => ({ ...prev, open: false }))
          setActiveSessionId(null)
        }, 2000)
        return
      }

      logProviderDecision('failure', {
        path: 'spaceAccessCompleted',
        sessionId,
        reason: reason ?? 'unknown',
      })
      setDialogState(prev => ({ ...prev, open: false }))
      toast.error(t('pairing.failed.title', { defaultValue: 'Pairing failed' }), {
        description: localizePairingError(reason),
      })
      setActiveSessionId(null)
    },
  })

  const handleCancel = () => {
    logProviderDecision('canceled', { path: 'dialog', sessionId: activeSessionIdRef.current })
    if (activeSessionIdRef.current && dialogState.peerId) {
      rejectP2PPairing(activeSessionIdRef.current, dialogState.peerId).catch(err => {
        const message = localizePairingError(err instanceof Error ? err.message : String(err))
        logProviderDecision('failure', {
          path: 'dialog.cancel_api',
          sessionId: activeSessionIdRef.current,
          reason: 'rejectP2PPairing_rejected',
        })
        log.error({ err }, 'Failed to cancel pairing dialog')
        toast.error(t('pairing.failed.title', { defaultValue: 'Pairing failed' }), {
          description: message,
        })
      })
    }
    setDialogState(prev => ({ ...prev, open: false }))
    setActiveSessionId(null)
  }

  return (
    <PairingPinDialog
      open={dialogState.open}
      onClose={handleCancel}
      pinCode={dialogState.pinCode}
      peerDeviceName={dialogState.peerDeviceName}
      isInitiator={false}
      onConfirm={matches => {
        if (!matches) {
          handleCancel()
        }
      }}
      phase={dialogState.phase}
    />
  )
}
