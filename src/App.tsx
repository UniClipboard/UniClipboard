import { LazyMotion, MotionConfig, domMax } from 'framer-motion'
import { use, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { BrowserRouter as Router, Route, Navigate, Outlet, useNavigate } from 'react-router-dom'
import { daemonClient } from '@/api/daemon/client'
import { signalLifecycleReady } from '@/api/daemon/lifecycle'
import { unlockEncryptionSession } from '@/api/security'
import { checkForUpdate, openUpdaterWindow } from '@/api/updater'
import { TitleBar } from '@/components'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import StartupModals from '@/components/StartupModals'
import { Button } from '@/components/ui/button'
import { Toaster } from '@/components/ui/sonner'
import { SearchProvider } from '@/contexts/SearchContext'
import { SettingProvider } from '@/contexts/SettingContext'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { TitleBarSlotContext } from '@/contexts/titlebar-slot-context'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { useEncryptionState } from '@/hooks/useDaemonEvents'
import { usePlatform } from '@/hooks/usePlatform'
import { useUINavigateListener } from '@/hooks/useUINavigateListener'
import { MainLayout, SettingsFullLayout, WindowShell } from '@/layouts'
import { DaemonBootstrapFailedError } from '@/lib/daemon-connection-info'
import {
  shouldSignalDaemonLifecycleReady,
  type EncryptionStatusView,
} from '@/lib/daemon-lifecycle-ready'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { commands, type DaemonBootstrapFailure } from '@/lib/ipc'
import { reportError } from '@/observability/errors'
import { SentryRoutes } from '@/observability/sentry'
import DevicesPage from '@/pages/DevicesPage'
import HistoryPage from '@/pages/HistoryPage'
import SettingsPage from '@/pages/SettingsPage'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import { useGetEncryptionSessionStatusQuery } from '@/store/api'
import { type SetupFlow, useSetupRealtimeStore } from '@/store/setupRealtimeStore'
import './App.css'

/** How long the initial encryption-status load may stay blank before the UI
 *  falls through to the actionable error screen instead of hanging (#995). */
const LOADING_WATCHDOG_MS = 12_000

// 认证布局包装器 - 保持 Sidebar 持久化
const AuthenticatedLayout = () => {
  return (
    <MainLayout>
      <Outlet />
    </MainLayout>
  )
}

/**
 * Returns true when the setup completion screen should remain visible after
 * the underlying flow has just transitioned to `completed`. We only latch
 * this if the previous flow was a non-completed state — that distinguishes
 * "just-finished pairing in this session" (show the success summary) from
 * "this device was already set up at launch" (skip straight into the app).
 */
export function shouldKeepSetupCompletionStep(
  previousFlow: SetupFlow | null,
  nextFlow: SetupFlow,
  hydrated: boolean
): boolean {
  return (
    hydrated &&
    previousFlow !== null &&
    previousFlow.kind !== 'completed' &&
    previousFlow.kind !== 'loading' &&
    nextFlow.kind === 'completed'
  )
}

export function isSetupGateActive(
  flow: SetupFlow,
  hydrated: boolean,
  showCompletionStep: boolean
): boolean {
  return !hydrated || flow.kind !== 'completed' || showCompletionStep
}

// 主应用程序内容
const AppContent = ({
  isSetupActive,
  onSetupComplete,
}: {
  isSetupActive: boolean
  onSetupComplete: () => void
}) => {
  const [encryptionStatus, setEncryptionStatus] = useState<EncryptionStatusView | null>(null)
  // Captures boot-time WS failures so the error UI can surface them even
  // before the RTK Query attempt fires. The final `encryptionError` view is
  // derived (see below) from this + the query error so we don't have to
  // chain a clear-on-success setEncryptionError(null) behind setEncryptionStatus.
  const [bootEncryptionError, setBootEncryptionError] = useState<string | null>(null)
  // Typed daemon-bootstrap failure (when the native side gave up reaching the
  // daemon), so the error screen can branch on `kind` — e.g. tell the user to
  // update the app on a version mismatch rather than just "restart".
  const [bootstrapFailure, setBootstrapFailure] = useState<DaemonBootstrapFailure | null>(null)
  const [daemonBootstrapReady, setDaemonBootstrapReady] = useState(false)
  const bootstrapRetryingRef = useRef(false)
  const daemonLifecycleReadySignaledRef = useRef(false)
  // Post-setup auto-unlock is handled by onSetupComplete callback (in AppContentWithBar),
  // NOT by detecting isSetupActive transitions. Detecting transitions here would false-trigger
  // on initial hydration: isSetupActive starts true (hydrated=false placeholder) then becomes
  // false when hydration completes with setupState='Completed', mimicking a setup→completed
  // transition even though setup was already done.

  useEffect(() => {
    if (isSetupActive) {
      return
    }

    let cancelled = false

    connectDaemonWs()
      .then(() => {
        if (!cancelled) {
          setDaemonBootstrapReady(true)
          setBootEncryptionError(null)
          setBootstrapFailure(null)
        }
      })
      .catch(error => {
        if (cancelled) return
        const message = error instanceof Error ? error.message : String(error)
        setBootEncryptionError(message)
        // Capture the typed failure so the error screen can give an
        // action-specific message (update vs restart).
        setBootstrapFailure(error instanceof DaemonBootstrapFailedError ? error.failure : null)
      })

    return () => {
      cancelled = true
    }
  }, [isSetupActive])

  const {
    data: encryptionData,
    isLoading: encryptionLoading,
    error: encryptionQueryError,
    refetch: refetchEncryption,
  } = useGetEncryptionSessionStatusQuery(undefined, {
    skip: isSetupActive || !daemonBootstrapReady,
  })

  // Watchdog: the daemon session token can expire while the keep-alive timer is
  // frozen across a sleep; if the initial encryption-status request then stalls
  // (e.g. a refresh that never resolves), the blank loading gate below would
  // trap the UI forever — the user had to restart the app to recover (#995).
  // After LOADING_WATCHDOG_MS without a status or error, surface the actionable
  // error screen (with Retry) instead of an indefinite blank screen.
  const [loadingTimedOut, setLoadingTimedOut] = useState(false)
  const isInitialLoading = encryptionLoading && encryptionStatus === null
  // Arm the watchdog while the initial load is in flight; disarm (clearTimeout)
  // when it ends. No state is adjusted here on the flag change — a stale
  // timed-out `true` once loading ends is harmless (any data/error overrides it
  // in the gates below), and the only way back into a loading cycle is the
  // Retry handler, which resets the flag explicitly.
  useEffect(() => {
    if (!isInitialLoading) return
    const id = setTimeout(() => setLoadingTimedOut(true), LOADING_WATCHDOG_MS)
    return () => clearTimeout(id)
  }, [isInitialLoading])

  // Listen for encryption session ready/failed via daemon WebSocket.
  useEncryptionState(
    () => {
      // Session became ready — update status without downgrading session_ready.
      setEncryptionStatus(prev =>
        prev ? { ...prev, session_ready: true } : { initialized: true, session_ready: true }
      )
    },
    () => {
      // Session failed — clear session_ready.
      setEncryptionStatus(prev =>
        prev ? { ...prev, session_ready: false } : { initialized: true, session_ready: false }
      )
    }
  )

  useEffect(() => {
    if (!encryptionData) return
    setEncryptionStatus(prev => {
      // Never downgrade session_ready from true → false.
      // The RTK Query result may be stale (captured before unlock completed),
      // so if we already know the session is ready (from a SessionReady event),
      // do not let an older query result roll that back.
      if (prev?.session_ready && !encryptionData.session_ready) {
        return prev
      }
      return encryptionData
    })
  }, [encryptionData])

  // Derive the encryptionError view directly from the RTK Query error +
  // any locally-captured boot error. Keeping this in a single useState +
  // useEffect would chain a clear-on-success state update behind the
  // success-path setEncryptionStatus above; collapsing it to a derived
  // value avoids that chain.
  const encryptionQueryErrorMessage = encryptionQueryError
    ? typeof encryptionQueryError === 'object' && 'message' in encryptionQueryError
      ? String(encryptionQueryError.message)
      : 'Failed to check encryption status'
    : null
  const encryptionError = encryptionData
    ? null
    : (bootEncryptionError ??
      encryptionQueryErrorMessage ??
      (loadingTimedOut ? 'Timed out waiting for the background service.' : null))

  const resolvedEncryptionStatus = encryptionStatus ?? encryptionData ?? null

  // Retry RESTARTS the daemon, then reconnects. A plain WS reconnect can never
  // recover from a daemon that is wedged or left over from a previous version
  // (the post-update "had to kill uniclipd by hand" case): restart_daemon stops
  // THIS profile's pid-file daemon, waits for it to exit, and spawns a fresh one
  // — whose instance-lock eviction reclaims the lock from any stuck holder. A
  // missing pid file is fine (nothing to stop → just start). We then re-run the
  // WS bootstrap, force a fresh session token, and re-pull encryption status.
  const handleBootstrapRetry = useCallback(() => {
    bootstrapRetryingRef.current = true
    setLoadingTimedOut(false)
    setBootEncryptionError(null)
    setBootstrapFailure(null)
    commands
      .restartDaemon()
      .then(() => connectDaemonWs())
      .then(() => {
        setDaemonBootstrapReady(true)
        return daemonClient.refreshSession()
      })
      .then(() => {
        void refetchEncryption()
      })
      .catch(error => {
        const message = error instanceof Error ? error.message : String(error)
        setBootEncryptionError(message)
        setBootstrapFailure(error instanceof DaemonBootstrapFailedError ? error.failure : null)
      })
      .finally(() => {
        bootstrapRetryingRef.current = false
      })
  }, [refetchEncryption])

  // Poll the native bootstrap status independently of isSetupActive. When the
  // daemon fails to start (e.g. iroh-blobs crash, port conflict), the native
  // side records a terminal failure after ~8s. Without this effect the failure
  // is invisible when the setup gate is still active — setupRealtimeStore
  // can't hydrate without a daemon, so isSetupActive stays true and the error
  // screen (gated behind !isSetupActive) never renders. This effect surfaces
  // the failure so we can show it above the setup gate.
  useEffect(() => {
    if (daemonBootstrapReady || bootstrapFailure) return

    let cancelled = false
    const id = setInterval(async () => {
      if (cancelled || bootstrapRetryingRef.current) return
      try {
        const failure = await commands.getDaemonBootstrapFailure()
        if (failure && !cancelled && !bootstrapRetryingRef.current) {
          setBootstrapFailure(failure)
          setBootEncryptionError(failure.detail || 'The background service failed to start.')
          reportError(new Error(`Daemon bootstrap failed: ${failure.kind}`), {
            kind: failure.kind,
            detail: failure.detail,
            observedVersion: failure.observedVersion,
            expectedVersion: failure.expectedVersion,
          })
        }
      } catch {
        // Best-effort; the Tauri command itself failing is non-fatal.
      }
    }, 1_000)

    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [daemonBootstrapReady, bootstrapFailure])

  useEffect(() => {
    if (
      daemonLifecycleReadySignaledRef.current ||
      !shouldSignalDaemonLifecycleReady(
        isSetupActive,
        daemonBootstrapReady,
        resolvedEncryptionStatus
      )
    ) {
      return
    }

    daemonLifecycleReadySignaledRef.current = true
    signalLifecycleReady().catch(error => {
      daemonLifecycleReadySignaledRef.current = false
      console.error('Failed to signal daemon lifecycle ready:', error)
    })
  }, [daemonBootstrapReady, isSetupActive, resolvedEncryptionStatus])

  // Daemon bootstrap failure takes precedence over every other gate — if the
  // daemon can't start, neither setup nor encryption can proceed. Show the
  // error screen regardless of isSetupActive so the user isn't stuck on a
  // perpetual setup-loading spinner when the daemon is broken.
  if (bootstrapFailure) {
    const versionTooOld = bootstrapFailure.kind === 'versionTooOld'
    return (
      <div className="flex h-full w-full items-center justify-center p-4 text-sm text-foreground">
        <div className="max-w-sm space-y-3 text-center">
          <p>
            {versionTooOld
              ? 'A newer version is already running in the background. Please update this app to continue.'
              : "Couldn't reach the background service. Please restart the app."}
          </p>
          <p className="break-words text-xs text-muted-foreground">
            {bootEncryptionError ?? bootstrapFailure.detail}
          </p>
          {versionTooOld ? (
            <Button
              size="sm"
              variant="outline"
              onClick={() => {
                void checkForUpdate(null).catch(error =>
                  console.error('Update check from bootstrap error screen failed:', error)
                )
                void openUpdaterWindow().catch(error =>
                  console.error('Failed to open updater window:', error)
                )
              }}
            >
              Open updater
            </Button>
          ) : (
            <Button size="sm" variant="outline" onClick={handleBootstrapRetry}>
              Retry
            </Button>
          )}
        </div>
      </div>
    )
  }

  if (isSetupActive) {
    return (
      <>
        <SetupPage onCompleteSetup={onSetupComplete} />
        <Toaster />
      </>
    )
  }

  // Only show blank screen during initial load when we have no encryption status at all.
  // Once encryptionStatus is known (from a previous query or SessionReady event), we continue
  // rendering even if RTK Query is re-fetching — this prevents a blank screen flash when
  // isSetupActive transitions from true→false and RTK Query starts a new request.
  // Stay blank only while genuinely still loading with no error. Once the
  // watchdog (or the query) produces an error, fall through to the error
  // screen below instead of short-circuiting to an indefinite blank (#995).
  if (encryptionLoading && encryptionStatus === null && !encryptionError) {
    return null
  }

  if (encryptionError) {
    return (
      <div className="flex h-full w-full items-center justify-center p-4 text-sm text-foreground">
        <div className="max-w-sm space-y-3 text-center">
          <p>Couldn&apos;t reach the background service. Please restart the app.</p>
          <p className="break-words text-xs text-muted-foreground">{encryptionError}</p>
          <Button size="sm" variant="outline" onClick={handleBootstrapRetry}>
            Retry
          </Button>
        </div>
      </div>
    )
  }

  if (!daemonBootstrapReady && encryptionStatus === null) {
    return null
  }

  // If initialized but not ready, show unlock page.
  if (resolvedEncryptionStatus?.initialized && !resolvedEncryptionStatus?.session_ready) {
    return (
      <>
        <UnlockPage
          onUnlockSucceeded={() => setEncryptionStatus({ initialized: true, session_ready: true })}
          onResetSucceeded={() => setEncryptionStatus({ initialized: false, session_ready: false })}
        />
      </>
    )
  }

  return (
    <>
      <GlobalShortcuts />
      <SentryRoutes>
        <Route element={<AuthenticatedLayout />}>
          <Route path="/" element={<Navigate to="/history" replace />} />
          <Route path="/history" element={<HistoryPage />} />
          <Route path="/devices" element={<DevicesPage />} />
        </Route>
        <Route element={<SettingsFullLayout />}>
          <Route path="/settings" element={<SettingsPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </SentryRoutes>
      <Toaster />
      <StartupModals />
    </>
  )
}

export default function App() {
  const { reduceVisualEffects } = usePlatform()

  return (
    <LazyMotion features={domMax} strict>
      <MotionConfig reducedMotion={reduceVisualEffects ? 'always' : 'user'}>
        <Router>
          <SearchProvider>
            <SettingProvider>
              <UpdateProvider>
                <AppContentWithBar />
              </UpdateProvider>
            </SettingProvider>
          </SearchProvider>
        </Router>
      </MotionConfig>
    </LazyMotion>
  )
}

// TitleBar wrapper with slot context
const TitleBarWithSearch = ({ isSetupActive }: { isSetupActive: boolean }) => {
  const slotCtx = use(TitleBarSlotContext)
  return <TitleBar isSetupActive={isSetupActive} rightSlot={slotCtx?.rightSlot} />
}

// App content with WindowShell structure
export const AppContentWithBar = () => {
  // WindowShell provides the correct window-level structure:
  // - TitleBar: Window chrome layer (full-width, drag region)
  // - Content: App layout layer (Sidebar + Main via routes)
  const { isMac, isTauri, isWindows } = usePlatform()
  const showCustomTitleBar = !isTauri || isMac || isWindows
  const { hydrated, flow } = useSetupRealtimeStore()
  const [showCompletionStep, setShowCompletionStep] = useState(false)
  const previousFlowRef = useRef<SetupFlow | null>(null)

  useEffect(() => {
    const previousFlow = previousFlowRef.current
    if (shouldKeepSetupCompletionStep(previousFlow, flow, hydrated)) {
      setShowCompletionStep(true)
    }
    previousFlowRef.current = flow
  }, [hydrated, flow])

  const isSetupActive = isSetupGateActive(flow, hydrated, showCompletionStep)

  const navigate = useNavigate()
  const handleNavigate = useCallback(
    (route: string) => {
      navigate(route)
    },
    [navigate]
  )
  useUINavigateListener(handleNavigate)

  const handleSetupComplete = () => {
    setShowCompletionStep(false)
    // When setup just completed, trigger Tauri-side auto-unlock.
    // Trigger Tauri-side auto-unlock only when setup actually completes during this session.
    // The daemon runs MarkSetupComplete + ensure_ready on its side, but the Tauri-side
    // encryption session needs its own unlock to become session_ready.
    unlockEncryptionSession().catch(err => console.warn('Post-setup auto-unlock failed:', err))
  }

  const [rightSlot, setRightSlot] = useState<React.ReactNode>(null)
  const slotValue = useMemo(() => ({ rightSlot, setRightSlot }), [rightSlot])

  // Memoize so WindowShell doesn't receive brand-new titleBar JSX every render
  // (jsx-no-jsx-as-prop) — only rebuild when its inputs actually change.
  const titleBar = useMemo(
    () =>
      showCustomTitleBar ? (
        <TitleBarSlotContext value={slotValue}>
          <TitleBarWithSearch isSetupActive={isSetupActive} />
        </TitleBarSlotContext>
      ) : null,
    [showCustomTitleBar, slotValue, isSetupActive]
  )

  return (
    <TitleBarSlotContext value={slotValue}>
      {/* ShortcutProvider wraps the whole shell (title bar + content) so the
          composite search box hoisted into the mac title bar slot still resolves
          the shortcut context — React context follows the render tree, and the
          slot renders inside the title bar, above AppContent. */}
      <ShortcutProvider>
        <WindowShell titleBar={titleBar}>
          <AppContent isSetupActive={isSetupActive} onSetupComplete={handleSetupComplete} />
        </WindowShell>
      </ShortcutProvider>
    </TitleBarSlotContext>
  )
}
