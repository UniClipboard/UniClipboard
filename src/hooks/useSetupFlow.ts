import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import {
  cancelInvitation,
  getSetupState,
  initializeSpace,
  issuePairingInvitation,
  redeemInvitation,
  resetSetup,
  SetupV2Error,
  type IssueInvitationErrorKind,
  type RedeemInvitationErrorKind,
  type InitializeSpaceErrorKind,
  type RedeemResponse,
} from '@/api/daemon/setupV2'
import { createLogger } from '@/lib/logger'
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
  /** S5 — both: post-handshake summary. `redeem` is set on the joiner side. */
  | { kind: 'pairing_complete'; role: 'sponsor' | 'joiner'; redeem?: RedeemResponse }

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
    | { ok: true; redeem: RedeemResponse }
    | { ok: false; kind: RedeemInvitationErrorKind; raw: string }
  >
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

  const screen: SetupScreen = (() => {
    if (flow.kind === 'loading') return { kind: 'loading' }
    if (flow.kind === 'invitation_pending') {
      return { kind: 'show_invitation', code: flow.code, expiresAtMs: flow.expiresAtMs }
    }
    if (flow.kind === 'completed' && flow.completion) {
      return flow.completion.role === 'joiner'
        ? { kind: 'pairing_complete', role: 'joiner', redeem: flow.completion.redeem }
        : { kind: 'pairing_complete', role: 'sponsor' }
    }
    if (pageScreen) return pageScreen
    return { kind: 'entry' }
  })()

  const goEntry = useCallback(() => setPageScreen({ kind: 'entry' }), [])
  const startCreateSpace = useCallback(() => setPageScreen({ kind: 'initialize_space' }), [])
  const startJoinSpace = useCallback(() => setPageScreen({ kind: 'redeem_invitation' }), [])
  const startImportConfig = useCallback(() => setPageScreen({ kind: 'import_config' }), [])

  const handleInitialize = useCallback(
    async (input: { passphrase: string; passphraseConfirm: string; deviceName: string }) => {
      setLoading(true)
      try {
        await initializeSpace({
          passphrase: input.passphrase,
          passphraseConfirm: input.passphraseConfirm,
          deviceName: input.deviceName,
        })
        // After initialize the device has hasCompleted=true with no pending
        // invitation — push the canonical state so the gate flips immediately,
        // then surface the post-pairing summary as the page screen so the
        // user gets visual confirmation before being dropped into the app.
        const next = await getSetupState()
        applyServerSetupState(next, { role: 'sponsor' })
        return { ok: true } as const
      } catch (err) {
        if (err instanceof SetupV2Error) {
          log.warn({ kind: err.kind, raw: err.raw }, 'initializeSpace failed')
          return { ok: false, kind: err.kind as InitializeSpaceErrorKind, raw: err.raw } as const
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
    try {
      const out = await issuePairingInvitation()
      // The response is already authoritative. The matching WebSocket event
      // may arrive before or after it, and both converge through the store's
      // single invitation transition.
      applyIssuedInvitation(out)
      return { ok: true } as const
    } catch (err) {
      if (err instanceof SetupV2Error) {
        log.warn({ kind: err.kind, raw: err.raw }, 'issuePairingInvitation failed')
        return { ok: false, kind: err.kind as IssueInvitationErrorKind, raw: err.raw } as const
      }
      log.error({ err }, 'issuePairingInvitation failed unexpectedly')
      toast.error(t('errors.operationFailed'))
      return { ok: false, kind: 'internal' as IssueInvitationErrorKind, raw: String(err) } as const
    } finally {
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
        const redeem = await redeemInvitation({ code: input.code, passphrase: input.passphrase })
        const next = await getSetupState()
        applyServerSetupState(next, { role: 'joiner', redeem })
        return { ok: true, redeem } as const
      } catch (err) {
        if (err instanceof SetupV2Error) {
          log.warn({ kind: err.kind, raw: err.raw }, 'redeemInvitation failed')
          return { ok: false, kind: err.kind as RedeemInvitationErrorKind, raw: err.raw } as const
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
    finishPairing,
    resetSetup: handleReset,
  }
}
