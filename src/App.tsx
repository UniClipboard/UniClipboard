import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import {
  BrowserRouter as Router,
  Routes,
  Route,
  Navigate,
  Outlet,
  useNavigate,
} from 'react-router-dom'
import { type EncryptionSessionStatus } from '@/api/security'
import { getSetupState, type SetupState } from '@/api/setup'
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
import './App.css'

// 认证布局包装器 - 保持 Sidebar 持久化
const AuthenticatedLayout = () => {
  return (
    <MainLayout>
      <Outlet />
    </MainLayout>
  )
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
      setEncryptionStatus(encryptionData)
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

  if (encryptionLoading) {
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

  // If initialized but not ready, show unlock page
  if (resolvedEncryptionStatus?.initialized && !resolvedEncryptionStatus?.session_ready) {
    return <UnlockPage />
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
const AppContentWithBar = () => {
  // WindowShell provides the correct window-level structure:
  // - TitleBar: Window chrome layer (full-width, drag region)
  // - Content: App layout layer (Sidebar + Main via routes)
  const { isMac, isTauri } = usePlatform()
  const showCustomTitleBar = !isTauri || isMac
  const [setupGate, setSetupGate] = useState<{
    setupState: SetupState | null
    showCompletionStep: boolean
  }>({
    setupState: null,
    showCompletionStep: false,
  })

  const isSetupLoading = setupGate.setupState === null
  const isSetupActive =
    isSetupLoading || setupGate.setupState !== 'Completed' || setupGate.showCompletionStep

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null
    let isMounted = true

    const loadSetupState = async () => {
      try {
        const state = await getSetupState()
        if (!isMounted) return
        setSetupGate(prev => {
          const shouldShowCompletionStep =
            prev.showCompletionStep ||
            (prev.setupState !== null && prev.setupState !== 'Completed' && state === 'Completed')
          return {
            setupState: state,
            showCompletionStep: shouldShowCompletionStep,
          }
        })
        if (state !== 'Completed') {
          timer = setTimeout(loadSetupState, 1000)
        }
      } catch (error) {
        console.error(error)
        if (!isMounted) return
        timer = setTimeout(loadSetupState, 2000)
      }
    }

    loadSetupState()

    return () => {
      isMounted = false
      if (timer) clearTimeout(timer)
    }
  }, [])

  const navigate = useNavigate()
  const handleNavigate = useCallback(
    (route: string) => {
      navigate(route)
    },
    [navigate]
  )
  useUINavigateListener(handleNavigate)

  const handleSetupComplete = () => {
    setSetupGate(prev => ({ ...prev, showCompletionStep: false }))
  }

  return (
    <WindowShell
      titleBar={showCustomTitleBar ? <TitleBarWithSearch isSetupActive={isSetupActive} /> : null}
    >
      <AppContent isSetupActive={isSetupActive} onSetupComplete={handleSetupComplete} />
    </WindowShell>
  )
}
