import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
  Outlet,
  useNavigate,
} from 'react-router-dom'
import { type EncryptionSessionStatus, unlockEncryptionSession } from '@/api/security'
import { type SetupState } from '@/api/setup'
import { TitleBar } from '@/components'
import { GlobalShortcuts } from '@/components/GlobalShortcuts'
import { PairingNotificationProvider } from '@/components/PairingNotificationProvider'
import { Toaster } from '@/components/ui/sonner'
import { useSearch } from '@/contexts/search-context'
import { SearchProvider } from '@/contexts/SearchContext'
import { SettingProvider } from '@/contexts/SettingContext'
import { ShortcutProvider } from '@/contexts/ShortcutContext'
import { UpdateProvider } from '@/contexts/UpdateContext'
import { usePlatform } from '@/hooks/usePlatform'
import { useUINavigateListener } from '@/hooks/useUINavigateListener'
import { MainLayout, SettingsFullLayout, WindowShell } from '@/layouts'
import DashboardPage from '@/pages/DashboardPage'
import DevicesPage from '@/pages/DevicesPage'
import SettingsPage from '@/pages/SettingsPage'
import SetupPage from '@/pages/SetupPage'
import UnlockPage from '@/pages/UnlockPage'
import { useGetEncryptionSessionStatusQuery } from '@/store/api'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'
import './App.css'

// 认证布局包装器 - 保持 Sidebar 持久化
const AuthenticatedLayout = () => {
  return (
    <MainLayout>
      <Outlet />
    </MainLayout>
  )
}

export function shouldKeepSetupCompletionStep(
  previousSetupState: SetupState | null,
  nextSetupState: SetupState | null,
  hydrated: boolean
): boolean {
  return (
    hydrated &&
    previousSetupState !== null &&
    previousSetupState !== 'Completed' &&
    nextSetupState === 'Completed'
  )
}

export function isSetupGateActive(
  setupState: SetupState | null,
  hydrated: boolean,
  showCompletionStep: boolean
): boolean {
  return !hydrated || setupState !== 'Completed' || showCompletionStep
}

// 主应用程序内容
const AppContent = ({
  isSetupActive,
  onSetupComplete,
}: {
  isSetupActive: boolean
  onSetupComplete: () => void
}) => {
  const [encryptionStatus, setEncryptionStatus] = useState<EncryptionSessionStatus | null>(null)
  const [encryptionError, setEncryptionError] = useState<string | null>(null)
  // Post-setup auto-unlock is handled by onSetupComplete callback (in AppContentWithBar),
  // NOT by detecting isSetupActive transitions. Detecting transitions here would false-trigger
  // on initial hydration: isSetupActive starts true (hydrated=false placeholder) then becomes
  // false when hydration completes with setupState='Completed', mimicking a setup→completed
  // transition even though setup was already done.

  const {
    data: encryptionData,
    isLoading: encryptionLoading,
    error: encryptionQueryError,
  } = useGetEncryptionSessionStatusQuery(undefined, { skip: isSetupActive })

  useEffect(() => {
    const unlistenPromise = listen<'SessionReady' | { type: string }>(
      'encryption://event',
      event => {
        console.log('Encryption event:', event.payload)
        const eventType = typeof event.payload === 'string' ? event.payload : event.payload?.type
        if (eventType === 'SessionReady') {
          setEncryptionStatus(prev =>
            prev ? { ...prev, session_ready: true } : { initialized: true, session_ready: true }
          )
        }
      }
    )

    return () => {
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [])

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

  if (isSetupActive) {
    return <SetupPage onCompleteSetup={onSetupComplete} />
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

  // If initialized but not ready, show unlock page.
  // PairingNotificationProvider is mounted here too so that already-completed
  // hosts can still receive and display pairing requests while on the unlock screen.
  if (resolvedEncryptionStatus?.initialized && !resolvedEncryptionStatus?.session_ready) {
    return (
      <>
        <UnlockPage />
        <PairingNotificationProvider />
      </>
    )
  }

  return (
    <ShortcutProvider>
      <GlobalShortcuts />
      <Routes>
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
      </Routes>
      <Toaster />
      <PairingNotificationProvider />
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
  const { isMac, isTauri } = usePlatform()
  const showCustomTitleBar = !isTauri || isMac
  const { hydrated, setupState } = useSetupRealtimeStore()
  const [showCompletionStep, setShowCompletionStep] = useState(false)
  const previousSetupStateRef = useRef<SetupState | null>(null)

  useEffect(() => {
    const previousSetupState = previousSetupStateRef.current
    if (shouldKeepSetupCompletionStep(previousSetupState, setupState, hydrated)) {
      setShowCompletionStep(true)
    }
    previousSetupStateRef.current = setupState
  }, [hydrated, setupState])

  const isSetupActive = isSetupGateActive(setupState, hydrated, showCompletionStep)

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
