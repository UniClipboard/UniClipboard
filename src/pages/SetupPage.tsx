import { AnimatePresence } from 'framer-motion'
import { Loader2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { getP2PPeers, P2PPeerInfo } from '@/api/p2p'
import {
  cancelSetup,
  getSetupState,
  confirmPeerTrust,
  onSetupStateChanged,
  selectJoinPeer,
  startJoinSpace,
  startNewSpace,
  submitPassphrase,
  verifyPassphrase,
  SetupState,
} from '@/api/setup'
import CreatePassphraseStep from '@/pages/setup/CreatePassphraseStep'
import JoinPickDeviceStep from '@/pages/setup/JoinPickDeviceStep'
import JoinVerifyPassphraseStep from '@/pages/setup/JoinVerifyPassphraseStep'
import PairingConfirmStep from '@/pages/setup/PairingConfirmStep'
import ProcessingJoinStep from '@/pages/setup/ProcessingJoinStep'
import SetupDoneStep from '@/pages/setup/SetupDoneStep'
import StepDotIndicator from '@/pages/setup/StepDotIndicator'
import WelcomeStep from '@/pages/setup/WelcomeStep'

type SetupPageProps = {
  onCompleteSetup?: () => void
}

function getStateOrdinal(state: SetupState | null): number {
  if (!state) return -1
  if (state === 'Welcome') return 0
  if (state === 'Completed') return 99
  if (typeof state === 'object') {
    if ('CreateSpaceInputPassphrase' in state) return 1
    if ('ProcessingCreateSpace' in state) return 2
    if ('JoinSpaceSelectDevice' in state) return 1
    if ('JoinSpaceInputPassphrase' in state) return 2
    if ('JoinSpaceConfirmPeer' in state) return 3
    if ('ProcessingJoinSpace' in state) return 4
  }
  return -1
}

function getStepInfo(state: SetupState | null): { total: number; current: number } | null {
  if (!state || state === 'Welcome') return null
  if (state === 'Completed') {
    // Completed can be reached from either flow; show final dot
    // We don't know which flow, so return null (no dots on done)
    return null
  }
  if (typeof state === 'object') {
    // Create flow: InputPassphrase(0) -> Processing(1) -> Done(2)
    if ('CreateSpaceInputPassphrase' in state) return { total: 3, current: 0 }
    if ('ProcessingCreateSpace' in state) return { total: 3, current: 1 }
    // Join flow: SelectDevice(0) -> InputPassphrase(1) -> ConfirmPeer(2) -> Processing(3) -> Done(4)
    if ('JoinSpaceSelectDevice' in state) return { total: 5, current: 0 }
    if ('JoinSpaceInputPassphrase' in state) return { total: 5, current: 1 }
    if ('JoinSpaceConfirmPeer' in state) return { total: 5, current: 2 }
    if ('ProcessingJoinSpace' in state) return { total: 5, current: 3 }
  }
  return null
}

export default function SetupPage({ onCompleteSetup }: SetupPageProps = {}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.page' })
  const { t: tCommon } = useTranslation(undefined, { keyPrefix: 'setup.common' })
  const navigate = useNavigate()
  const [setupState, setSetupState] = useState<SetupState | null>(null)
  const [loading, setLoading] = useState(false)
  const [peers, setPeers] = useState<Array<{ id: string; name: string; device_type: string }>>([])
  const [peersLoading, setPeersLoading] = useState(false)
  const [isScanningInitial, setIsScanningInitial] = useState(true)
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null)
  const activeEventSessionIdRef = useRef<string | null>(null)
  const setupStateRef = useRef<SetupState | null>(null)
  const prevStateRef = useRef<SetupState | null>(null)

  const direction = useMemo(() => {
    return getStateOrdinal(setupState) >= getStateOrdinal(prevStateRef.current)
      ? 'forward'
      : 'backward'
  }, [setupState])

  useEffect(() => {
    prevStateRef.current = setupState
  }, [setupState])

  const stepInfo = useMemo(() => getStepInfo(setupState), [setupState])

  const isSetupFlowActive = useCallback((state: SetupState | null) => {
    if (!state) return false
    return state !== 'Welcome' && state !== 'Completed'
  }, [])

  const syncSetupState = useCallback((nextState: SetupState) => {
    setSetupState(prevState => {
      if (JSON.stringify(prevState) === JSON.stringify(nextState)) {
        return prevState
      }
      return nextState
    })
  }, [])

  useEffect(() => {
    const loadState = async () => {
      try {
        const state = await getSetupState()
        syncSetupState(state)
      } catch (error) {
        console.error('Failed to load setup state:', error)
        toast.error(t('errors.loadSetupStateFailed'))
      }
    }
    loadState()
  }, [syncSetupState, t])

  useEffect(() => {
    setupStateRef.current = setupState
  }, [setupState])

  useEffect(() => {
    let effectActive = true
    let disposed = false
    let unlisten: (() => void) | null = null

    const setupListener = async () => {
      const stopListening = await onSetupStateChanged(event => {
        if (!effectActive) {
          return
        }

        if (!isSetupFlowActive(setupStateRef.current)) {
          return
        }

        if (!activeEventSessionIdRef.current) {
          activeEventSessionIdRef.current = event.sessionId
        }

        if (activeEventSessionIdRef.current !== event.sessionId) {
          return
        }

        syncSetupState(event.state)

        if (event.state === 'Completed' || event.state === 'Welcome') {
          activeEventSessionIdRef.current = null
        }
      })

      if (disposed) {
        stopListening()
        return
      }

      unlisten = stopListening
    }

    void setupListener()

    return () => {
      effectActive = false
      disposed = true
      activeEventSessionIdRef.current = null
      if (unlisten) {
        unlisten()
        unlisten = null
      }
    }
  }, [isSetupFlowActive, syncSetupState])

  useEffect(() => {
    if (!isSetupFlowActive(setupState)) {
      activeEventSessionIdRef.current = null
    }
  }, [isSetupFlowActive, setupState])

  const handleRefreshPeers = useCallback(async () => {
    setPeersLoading(true)
    try {
      const peerList = await getP2PPeers()
      setPeers(
        peerList.map((p: P2PPeerInfo) => ({
          id: p.peerId,
          name: p.deviceName || tCommon('unknownDevice'),
          device_type: 'desktop',
        }))
      )
    } catch (error) {
      console.error('Failed to refresh peers:', error)
      toast.error(t('errors.refreshPeersFailed'))
    } finally {
      setPeersLoading(false)
      setIsScanningInitial(false)
    }
  }, [t, tCommon])

  useEffect(() => {
    if (setupState && typeof setupState === 'object' && 'JoinSpaceSelectDevice' in setupState) {
      handleRefreshPeers()
      const interval = setInterval(handleRefreshPeers, 3000)
      return () => {
        clearInterval(interval)
      }
    } else {
      setIsScanningInitial(true)
    }
  }, [setupState, handleRefreshPeers])

  const runAction = async (action: () => Promise<SetupState>) => {
    setLoading(true)
    try {
      const newState = await action()
      syncSetupState(newState)
    } catch (error) {
      console.error('Failed to dispatch event:', error)
      toast.error(t('errors.operationFailed'))
    } finally {
      setLoading(false)
    }
  }

  const renderStep = () => {
    if (!setupState) {
      return (
        <div className="flex h-full w-full items-center justify-center">
          <div className="flex items-center gap-3 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t('loadingSetupState')}
          </div>
        </div>
      )
    }

    if (setupState === 'Welcome') {
      return (
        <WelcomeStep
          onCreate={() => runAction(() => startNewSpace())}
          onJoin={() => runAction(() => startJoinSpace())}
          loading={loading}
          direction={direction}
        />
      )
    }

    if (setupState === 'Completed') {
      return (
        <SetupDoneStep
          onComplete={() => {
            onCompleteSetup?.()
            navigate('/', { replace: true })
          }}
          loading={loading}
          direction={direction}
        />
      )
    }

    if (typeof setupState === 'object') {
      if ('CreateSpaceInputPassphrase' in setupState) {
        return (
          <CreatePassphraseStep
            onSubmit={(pass1: string, pass2: string) =>
              runAction(() => submitPassphrase(pass1, pass2))
            }
            onBack={() => runAction(() => cancelSetup())}
            error={setupState.CreateSpaceInputPassphrase.error}
            loading={loading}
            direction={direction}
          />
        )
      }

      if ('JoinSpaceSelectDevice' in setupState) {
        return (
          <JoinPickDeviceStep
            onSelectPeer={(peerId: string) => {
              setSelectedPeerId(peerId)
              runAction(() => selectJoinPeer(peerId))
            }}
            onBack={() => runAction(() => cancelSetup())}
            onRefresh={handleRefreshPeers}
            peers={peers}
            error={setupState.JoinSpaceSelectDevice.error}
            loading={loading || peersLoading}
            isScanningInitial={isScanningInitial}
            direction={direction}
          />
        )
      }

      if ('JoinSpaceInputPassphrase' in setupState) {
        const { error } = setupState.JoinSpaceInputPassphrase
        return (
          <JoinVerifyPassphraseStep
            peerId={selectedPeerId ?? undefined}
            onSubmit={(passphrase: string) => runAction(() => verifyPassphrase(passphrase))}
            onBack={() => runAction(() => cancelSetup())}
            onCreateNew={() => runAction(() => startNewSpace())}
            error={error}
            loading={loading}
            direction={direction}
          />
        )
      }

      if ('JoinSpaceConfirmPeer' in setupState) {
        const { short_code, peer_fingerprint, error } = setupState.JoinSpaceConfirmPeer
        return (
          <PairingConfirmStep
            shortCode={short_code}
            peerFingerprint={peer_fingerprint}
            onConfirm={() => runAction(() => confirmPeerTrust())}
            onCancel={() => runAction(() => cancelSetup())}
            error={error}
            loading={loading}
            direction={direction}
          />
        )
      }

      if ('ProcessingCreateSpace' in setupState) {
        const message = setupState.ProcessingCreateSpace.message
        return (
          <div className="flex h-full w-full items-center justify-center">
            <div className="flex items-center gap-3 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              {message ?? t('processing')}
            </div>
          </div>
        )
      }

      if ('ProcessingJoinSpace' in setupState) {
        return (
          <ProcessingJoinStep
            onCancel={() => runAction(() => cancelSetup())}
            loading={loading}
            direction={direction}
          />
        )
      }
    }

    return (
      <div className="break-all text-sm text-muted-foreground">
        {t('unknownState', { state: JSON.stringify(setupState) })}
      </div>
    )
  }

  const stepKey = useMemo(() => {
    if (!setupState) return 'loading'
    if (typeof setupState === 'string') return setupState
    return Object.keys(setupState)[0] ?? 'unknown'
  }, [setupState])

  return (
    <div className="relative h-full w-full overflow-hidden bg-background">
      <div className="pointer-events-none absolute inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/20" />
        <div className="absolute -top-32 -left-32 h-96 w-96 bg-primary/5 blur-3xl" />
        <div className="absolute -bottom-32 -right-32 h-96 w-96 bg-emerald-500/5 blur-3xl" />
      </div>

      <div className="relative flex h-full w-full min-h-0 flex-col">
        <main
          className={`flex min-h-0 flex-1 items-center px-8 py-4 sm:px-12 sm:py-6 ${
            stepKey === 'Welcome' ? 'overflow-hidden' : 'overflow-y-auto'
          }`}
        >
          <div className="mx-auto w-full max-w-3xl max-h-full">
            <div className="max-h-full px-1 py-1 sm:px-0 sm:py-2">
              <AnimatePresence mode="wait" initial={false}>
                <div key={stepKey} className="w-full">
                  {renderStep()}
                </div>
              </AnimatePresence>
            </div>
          </div>
        </main>

        {stepInfo && (
          <div className="flex justify-center pb-4">
            <StepDotIndicator totalSteps={stepInfo.total} currentStep={stepInfo.current} />
          </div>
        )}
      </div>
    </div>
  )
}
