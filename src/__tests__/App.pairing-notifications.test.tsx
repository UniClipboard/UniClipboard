/**
 * @vitest-environment jsdom
 */

import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AppContentWithBar } from '@/App'

const usePlatformMock = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/lifecycle', () => ({
  signalLifecycleReady: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/api/security', () => ({
  unlockEncryptionSession: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/components', () => ({
  TitleBar: () => <div data-testid="title-bar" />,
}))

vi.mock('@/components/GlobalShortcuts', () => ({
  GlobalShortcuts: () => null,
}))

vi.mock('@/components/PairingNotificationProvider', () => ({
  PairingNotificationProvider: () => <div data-testid="pairing-notification-provider" />,
}))

vi.mock('@/components/ui/sonner', () => ({
  Toaster: () => <div data-testid="toaster" />,
}))

vi.mock('@/contexts/search-context', () => ({
  useSearch: () => ({
    searchValue: '',
    setSearchValue: vi.fn(),
  }),
}))

vi.mock('@/hooks/useDaemonEvents', () => ({
  useEncryptionState: vi.fn(),
}))

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: usePlatformMock,
}))

vi.mock('@/hooks/useUINavigateListener', () => ({
  useUINavigateListener: vi.fn(),
}))

vi.mock('@/layouts', () => ({
  MainLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SettingsFullLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  WindowShell: ({
    children,
    titleBar,
  }: {
    children: React.ReactNode
    titleBar?: React.ReactNode
  }) => (
    <div>
      {titleBar}
      {children}
    </div>
  ),
}))

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn(() => Promise.resolve()),
}))

vi.mock('@/pages/DashboardPage', () => ({
  default: () => <div data-testid="dashboard-page" />,
}))

vi.mock('@/pages/DevicesPage', () => ({
  default: () => <div data-testid="devices-page" />,
}))

vi.mock('@/pages/SettingsPage', () => ({
  default: () => <div data-testid="settings-page" />,
}))

vi.mock('@/pages/SetupPage', () => ({
  default: () => <div data-testid="setup-page" />,
}))

vi.mock('@/pages/UnlockPage', () => ({
  default: () => <div data-testid="unlock-page" />,
}))

vi.mock('@/store/api', () => ({
  useGetEncryptionSessionStatusQuery: vi.fn(() => ({
    data: null,
    isLoading: false,
    error: null,
  })),
}))

const useSetupRealtimeStoreMock = vi.hoisted(() => vi.fn())
vi.mock('@/store/setupRealtimeStore', () => ({
  useSetupRealtimeStore: useSetupRealtimeStoreMock,
}))

describe('App pairing notifications', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    usePlatformMock.mockReturnValue({ isMac: false, isTauri: false, isWindows: false })
    useSetupRealtimeStoreMock.mockReturnValue({
      hydrated: true,
      setupState: {
        JoinSpaceSelectDevice: {
          error: null,
        },
      },
      syncSetupStateFromCommand: vi.fn(),
    })
  })

  it('keeps pairing notifications mounted while setup gate is active', () => {
    render(
      <MemoryRouter>
        <AppContentWithBar />
      </MemoryRouter>
    )

    expect(screen.getByTestId('setup-page')).toBeInTheDocument()
    expect(screen.getByTestId('pairing-notification-provider')).toBeInTheDocument()
    expect(screen.getByTestId('toaster')).toBeInTheDocument()
  })

  it('renders the custom title bar on Windows Tauri', () => {
    usePlatformMock.mockReturnValue({ isMac: false, isTauri: true, isWindows: true })

    render(
      <MemoryRouter>
        <AppContentWithBar />
      </MemoryRouter>
    )

    expect(screen.getByTestId('title-bar')).toBeInTheDocument()
  })
})
