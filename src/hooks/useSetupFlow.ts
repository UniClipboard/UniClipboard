import { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { getDeviceTrustSnapshot } from '@/api/daemon/device-trust'
import {
  cancelJoinSpace,
  cancelInvitation,
  getSetupState,
  initializeSpace,
  issuePairingInvitation,
  redeemInvitation,
  resetSetup,
  SetupV2Error,
  type IssueInvitationErrorKind,
  type ActiveJoinSpaceResponse,
  type RedeemInvitationErrorKind,
  type InitializeSpaceErrorKind,
  type JoinSpaceRejectionReason,
} from '@/api/daemon/setupV2'
import { activeDeviceIds, findNewActiveDeviceId } from '@/components/device/pairing-success-utils'
import { toast } from '@/components/ui/toast'
import { type JoinAdmissionResolution, useJoinAdmission } from '@/hooks/useJoinAdmission'
import { daemonWs } from '@/lib/daemon-ws'
import { createLogger } from '@/lib/logger'
import { recordWdioE2eEvent } from '@/lib/wdio-test-bridge'
import {
  acknowledgeSetupCompletion,
  applyIssuedInvitation,
  applyServerSetupState,
  refreshSetupState,
  type SetupFlow,
  useSetupRealtimeStore,
} from '@/store/setupRealtimeStore'

const log = createLogger('use-setup-flow')

/**
 * Page-level screens visible inside the setup gate. The store-level
 * `SetupFlow` only carries enough state to decide which entry/recovery
 * screen to show on launch; navigating between intermediate forms is the
 * page hook's responsibility.
 */
export type SetupScreen =
  | { kind: 'loading' }
  /** S0 — choose create / join / import. */
  | { kind: 'entry' }
  /** S1 — sponsor: device name + passphrase + confirm. */
  | { kind: 'initialize_space' }
  /** S6 — migrate an existing setup from an exported bundle. */
  | { kind: 'import_config' }
  /** S3 — sponsor: showing invitation code with countdown. */
  | { kind: 'show_invitation'; code: string; expiresAtMs: number }
  /** S4 — joiner: paste invitation code + passphrase. */
  | { kind: 'redeem_invitation' }
  /** S4a — joiner: durable admission is waiting for its final outcome. */
  | { kind: 'join_pending'; joinId: string }
  /** S4b — joiner: durable admission was rejected. */
  | { kind: 'join_rejected'; reason: JoinSpaceRejectionReason }
  /** Sponsor Space is ready and can issue its first invitation. */
  | { kind: 'space_ready' }
  /** S5 — both: post-handshake summary. */
  | {
      kind: 'pairing_complete'
      localDeviceName: string | null
      peerDeviceId?: string | null
    }

export interface UseSetupFlowReturn {
  screen: SetupScreen
  flow: SetupFlow
  loading: boolean
  goEntry: () => void
  startCreateSpace: () => void
  startJoinSpace: () => void
  startImportConfig: () => void
  initializeSpace: (input: {
    passphrase: string
    passphraseConfirm: string
    deviceName: string
  }) => Promise<{ ok: true } | { ok: false; kind: InitializeSpaceErrorKind; raw: string }>
  issueInvitation: () => Promise<
    { ok: true } | { ok: false; kind: IssueInvitationErrorKind; raw: string }
  >
  cancelInvitation: () => Promise<void>
  redeemInvitation: (input: {
    code: string
    passphrase: string
  }) => Promise<
    | { ok: true; redeem: ActiveJoinSpaceResponse | null }
    | { ok: false; kind: RedeemInvitationErrorKind; raw: string }
  >
  cancelJoin: (joinId: string) => Promise<void>
  finishPairing: () => void
  resetSetup: () => Promise<void>
}

/**
 * Drives the setup gate UI. Backed by `useSetupRealtimeStore` for the base
 * flow state; layers a small page-screen state machine on top so the
 * intermediate forms (initialize, redeem) survive within a session without
 * polluting the store.
 */
export function useSetupFlow(): UseSetupFlowReturn {
  const { flow } = useSetupRealtimeStore()
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.page' })
  const [pageScreen, setPageScreen] = useState<SetupScreen | null>(null)
  const [loading, setLoading] = useState(false)
  const invitationDeviceIdsRef = useRef<ReadonlySet<string> | null>(null)
  const sponsorCompletionInFlightRef = useRef(false)

  const screen: SetupScreen = (() => {
    if (flow.kind === 'loading') return { kind: 'loading' }
    if (flow.kind === 'invitation_pending') {
      return {
        kind: 'show_invitation',
        code: flow.code,
        expiresAtMs: flow.expiresAtMs,
      }
    }
    if (flow.kind === 'completed' && flow.completion) {
      if (flow.completion.kind === 'space_ready') return { kind: 'space_ready' }
      return flow.completion.role === 'joiner'
        ? {
            kind: 'pairing_complete',
            localDeviceName: flow.deviceName,
            peerDeviceId: flow.completion.redeem.joinedSpace.sponsorDeviceId,
          }
        : {
            kind: 'pairing_complete',
            localDeviceName: flow.deviceName,
            peerDeviceId: flow.completion.peerDeviceId,
          }
    }
    if (pageScreen) return pageScreen
    return { kind: 'entry' }
  })()

  const goEntry = useCallback(() => setPageScreen({ kind: 'entry' }), [])
  const startCreateSpace = useCallback(() => setPageScreen({ kind: 'initialize_space' }), [])
  const startJoinSpace = useCallback(() => setPageScreen({ kind: 'redeem_invitation' }), [])
  const startImportConfig = useCallback(() => setPageScreen({ kind: 'import_config' }), [])

  const confirmSponsorPairing = useEffectEvent(async () => {
    if (
      flow.kind !== 'invitation_pending' ||
      invitationDeviceIdsRef.current === null ||
      sponsorCompletionInFlightRef.current
    ) {
      return
    }
    try {
      const trust = await getDeviceTrustSnapshot()
      const peerDeviceId = findNewActiveDeviceId(
        invitationDeviceIdsRef.current,
        activeDeviceIds(trust)
      )
      if (peerDeviceId === null) return
      sponsorCompletionInFlightRef.current = true
      try {
        await cancelInvitation()
      } catch (err) {
        log.warn({ err }, 'failed to clear completed sponsor invitation')
      }
      const state = await getSetupState()
      applyServerSetupState(
        { ...state, currentInvitation: null },
        {
          kind: 'pairing_succeeded',
          role: 'sponsor',
          sponsorDeviceId: trust.localDeviceId,
          peerDeviceId,
        }
      )
    } catch (err) {
      log.warn({ err }, 'failed to verify completed sponsor invitation')
      sponsorCompletionInFlightRef.current = false
    }
  })

  const resolveJoinAdmission = useCallback(async (result: JoinAdmissionResolution) => {
    if (result.status === 'rejected') {
      setPageScreen({ kind: 'join_rejected', reason: result.reason })
      return
    }
    try {
      setPageScreen(null)
      const next = await getSetupState()
      applyServerSetupState(next, {
        kind: 'pairing_succeeded',
        role: 'joiner',
        redeem: result,
      })
    } catch (err) {
      log.warn({ err }, 'failed to apply completed durable admission')
    }
  }, [])

  const pendingJoinId = pageScreen?.kind === 'join_pending' ? pageScreen.joinId : null
  useJoinAdmission(pendingJoinId, resolveJoinAdmission)

  useEffect(() => {
    if (flow.kind !== 'invitation_pending') return
    const unsubscribeDeviceTrust = daemonWs.subscribe(['device-trust', 'system'], event => {
      if (
        event.eventType === 'device-trust.changed' ||
        event.eventType === 'system.refresh_required'
      ) {
        void confirmSponsorPairing()
      }
    })
    const unsubscribeReconnect = daemonWs.onReconnect(() => void confirmSponsorPairing())
    return () => {
      unsubscribeDeviceTrust()
      unsubscribeReconnect()
    }
  }, [flow.kind])

  const handleInitialize = useCallback(
    async (input: { passphrase: string; passphraseConfirm: string; deviceName: string }) => {
      setLoading(true)
      try {
        await initializeSpace({
          passphrase: input.passphrase,
          passphraseConfirm: input.passphraseConfirm,
          deviceName: input.deviceName,
        })
        // Space initialization and peer pairing are separate milestones. Keep
        // setup open on the ready screen until the user invites a peer or exits.
        const next = await getSetupState()
        applyServerSetupState(next, { kind: 'space_ready' })
        return { ok: true } as const
      } catch (err) {
        if (err instanceof SetupV2Error) {
          log.warn({ kind: err.kind, raw: err.raw }, 'initializeSpace failed')
          return {
            ok: false,
            kind: err.kind as InitializeSpaceErrorKind,
            raw: err.raw,
          } as const
        }
        log.error({ err }, 'initializeSpace failed unexpectedly')
        toast.error(t('errors.operationFailed'))
        return {
          ok: false,
          kind: 'internal' as InitializeSpaceErrorKind,
          raw: String(err),
        } as const
      } finally {
        setLoading(false)
      }
    },
    [t]
  )

  const handleIssue = useCallback(async () => {
    setLoading(true)
    recordWdioE2eEvent('setup.issue.started')
    try {
      invitationDeviceIdsRef.current = activeDeviceIds(await getDeviceTrustSnapshot())
      const out = await issuePairingInvitation()
      recordWdioE2eEvent('setup.issue.returned')
      // The response is already authoritative. The matching WebSocket event
      // may arrive before or after it, and both converge through the store's
      // single invitation transition.
      applyIssuedInvitation(out)
      recordWdioE2eEvent('setup.issue.applied')
      return { ok: true } as const
    } catch (err) {
      recordWdioE2eEvent('setup.issue.failed', String(err))
      if (err instanceof SetupV2Error) {
        log.warn({ kind: err.kind, raw: err.raw }, 'issuePairingInvitation failed')
        return {
          ok: false,
          kind: err.kind as IssueInvitationErrorKind,
          raw: err.raw,
        } as const
      }
      log.error({ err }, 'issuePairingInvitation failed unexpectedly')
      toast.error(t('errors.operationFailed'))
      return {
        ok: false,
        kind: 'internal' as IssueInvitationErrorKind,
        raw: String(err),
      } as const
    } finally {
      recordWdioE2eEvent('setup.issue.finished')
      setLoading(false)
    }
  }, [t])

  const handleCancel = useCallback(async () => {
    setLoading(true)
    try {
      await cancelInvitation()
      await refreshSetupState()
      setPageScreen(null)
    } catch (err) {
      if (err instanceof SetupV2Error && err.kind === 'not_issued') {
        // Race: ws revoked already cleaned up. Fall through to refresh.
        await refreshSetupState()
        setPageScreen(null)
      } else {
        log.error({ err }, 'cancelInvitation failed')
        toast.error(t('errors.operationFailed'))
      }
    } finally {
      setLoading(false)
    }
  }, [t])

  const handleRedeem = useCallback(
    async (input: { code: string; passphrase: string }) => {
      setLoading(true)
      try {
        const redeem = await redeemInvitation({
          code: input.code,
          passphrase: input.passphrase,
        })
        if (redeem.status === 'pending') {
          setPageScreen({ kind: 'join_pending', joinId: redeem.joinId })
          return { ok: true, redeem: null } as const
        }
        if (redeem.status === 'rejected') {
          setPageScreen({ kind: 'join_rejected', reason: redeem.reason })
          return {
            ok: false,
            kind: 'internal' as RedeemInvitationErrorKind,
            raw: redeem.reason,
          } as const
        }
        const next = await getSetupState()
        applyServerSetupState(next, {
          kind: 'pairing_succeeded',
          role: 'joiner',
          redeem,
        })
        return { ok: true, redeem } as const
      } catch (err) {
        if (err instanceof SetupV2Error) {
          log.warn({ kind: err.kind, raw: err.raw }, 'redeemInvitation failed')
          return {
            ok: false,
            kind: err.kind as RedeemInvitationErrorKind,
            raw: err.raw,
          } as const
        }
        log.error({ err }, 'redeemInvitation failed unexpectedly')
        toast.error(t('errors.operationFailed'))
        return {
          ok: false,
          kind: 'internal' as RedeemInvitationErrorKind,
          raw: String(err),
        } as const
      } finally {
        setLoading(false)
      }
    },
    [t]
  )

  const handleCancelJoin = useCallback(
    async (joinId: string) => {
      setLoading(true)
      try {
        const result = await cancelJoinSpace(joinId)
        if (result.status === 'active') {
          await resolveJoinAdmission(result)
        } else if (result.status === 'rejected') {
          await resolveJoinAdmission(result)
        }
      } catch (err) {
        log.error({ err, joinId }, 'cancelJoinSpace failed')
        toast.error(t('errors.operationFailed'))
      } finally {
        setLoading(false)
      }
    },
    [t]
  )

  const handleReset = useCallback(async () => {
    setLoading(true)
    try {
      await resetSetup()
      await refreshSetupState()
      setPageScreen({ kind: 'entry' })
    } catch (err) {
      log.error({ err }, 'resetSetup failed')
      toast.error(t('errors.operationFailed'))
    } finally {
      setLoading(false)
    }
  }, [t])

  const finishPairing = useCallback(() => {
    acknowledgeSetupCompletion()
  }, [])

  return {
    screen,
    flow,
    loading,
    goEntry,
    startCreateSpace,
    startJoinSpace,
    startImportConfig,
    initializeSpace: handleInitialize,
    issueInvitation: handleIssue,
    cancelInvitation: handleCancel,
    redeemInvitation: handleRedeem,
    cancelJoin: handleCancelJoin,
    finishPairing,
    resetSetup: handleReset,
  }
}
