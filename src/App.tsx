import { useCallback, useEffect, useRef, useState } from 'react'
import { BrowserRouter as Router, Route, Navigate, Outlet, useNavigate } from 'react-router-dom'
import { signalLifecycleReady } from '@/api/daemon/lifecycle'
import { unlockEncryptionSession } from '@/api/security'
import { TitleBar } from '@/components'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import StartupModals from '@/components/StartupModals'
import { Toaster } from '@/components/ui/sonner'
import { useSearch } from '@/contexts/search-context'
import { SearchProvider } from '@/contexts/SearchContext'
import { SettingProvider } from '@/contexts/SettingContext'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { useEncryptionState } from '@/hooks/useDaemonEvents'
import { usePlatform } from '@/hooks/usePlatform'
import { useUINavigateListener } from '@/hooks/useUINavigateListener'
import { MainLayout, SettingsFullLayout, WindowShell } from '@/layouts'
import {
  shouldSignalDaemonLifecycleReady,
  type EncryptionStatusView,
} from '@/lib/daemon-lifecycle-ready'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import { SentryRoutes } from '@/observability/sentry'
import DashboardPage from '@/pages/DashboardPage'
import DevicesPage from '@/pages/DevicesPage'
import SettingsPage from '@/pages/SettingsPage'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import { useGetEncryptionSessionStatusQuery } from '@/store/api'
import { type SetupFlow, useSetupRealtimeStore } from '@/store/setupRealtimeStore'
import './App.css'

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
  const [encryptionError, setEncryptionError] = useState<string | null>(null)
  const [daemonBootstrapReady, setDaemonBootstrapReady] = useState(false)
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
          setEncryptionError(null)
        }
      })
      .catch(error => {
        if (!cancelled) {
          const message = error instanceof Error ? error.message : String(error)
          setEncryptionError(message)
        }
      })

    return () => {
      cancelled = true
    }
  }, [isSetupActive])

  const {
    data: encryptionData,
    isLoading: encryptionLoading,
    error: encryptionQueryError,
  } = useGetEncryptionSessionStatusQuery(undefined, {
    skip: isSetupActive || !daemonBootstrapReady,
  })

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
    if (encryptionData) {
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
      setEncryptionError(null)
    }
  }, [encryptionData])

  useEffect(() => {
    if (!encryptionQueryError) {
      return
    }

    const message =
      typeof encryptionQueryError === 'object' && 'message' in encryptionQueryError
        ? String(encryptionQueryError.message)
        : 'Failed to check encryption status'
    setEncryptionError(message)
  }, [encryptionQueryError])

  const resolvedEncryptionStatus = encryptionStatus ?? encryptionData ?? null

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
  if (encryptionLoading && encryptionStatus === null) {
    return null
  }

  if (encryptionError) {
    return (
      <div className="flex h-full w-full items-center justify-center p-4 text-sm text-foreground">
        <div className="max-w-sm rounded-md border border-border/20 bg-muted p-4 text-center">
          Failed to verify encryption status. Please restart the app.
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
        />
      </>
    )
  }

  return (
    <ShortcutProvider>
      <GlobalShortcuts />
      <SentryRoutes>
        <Route element={<AuthenticatedLayout />}>
          <Route
            path="/"
            element={
              <div className="w-full h-full">
                <DashboardPage />
              </div>
            }
          />
          <Route path="/devices" element={<DevicesPage />} />
        </Route>
        <Route element={<SettingsFullLayout />}>
          <Route path="/settings" element={<SettingsPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </SentryRoutes>
      <Toaster />
      <StartupModals />
    </ShortcutProvider>
  )
}

export default function App() {
  return (
    <Router>
      <SearchProvider>
        <SettingProvider>
          <UpdateProvider>
            <AppContentWithBar />
          </UpdateProvider>
        </SettingProvider>
      </SearchProvider>
    </Router>
  )
}

// TitleBar wrapper with search context
const TitleBarWithSearch = ({ isSetupActive }: { isSetupActive: boolean }) => {
  const { searchValue, setSearchValue } = useSearch()
  return (
    <TitleBar
      searchValue={searchValue}
      onSearchChange={setSearchValue}
      isSetupActive={isSetupActive}
    />
  )
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

  return (
    <WindowShell
      titleBar={showCustomTitleBar ? <TitleBarWithSearch isSetupActive={isSetupActive} /> : null}
    >
      <AppContent isSetupActive={isSetupActive} onSetupComplete={handleSetupComplete} />
    </WindowShell>
  )
}
