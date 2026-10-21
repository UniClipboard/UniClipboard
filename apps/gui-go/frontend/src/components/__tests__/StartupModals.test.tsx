import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import StartupModals from '@/components/StartupModals'

const mockSetupSnapshot = vi.hoisted(() => ({ rePairingRequired: false }))

// Existing isolated scenarios have no pending import; real rejection coverage is daemon E2E.
vi.mock('@/hooks/useConfigImportStatus', () => ({
  useConfigImportStatus: () => ({
    data: { rejectedImport: null },
    isError: false,
    refetch: vi.fn(),
  }),
}))
vi.mock('@/api/daemon/config-import', () => ({
  getConfigImportStatus: vi.fn().mockResolvedValue({ rejectedImport: null }),
}))

vi.mock('@/store/setupRealtimeStore', () => ({
  useSetupRealtimeStore: () => mockSetupSnapshot,
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

function LocationProbe() {
  return <span data-testid="location">{useLocation().pathname}</span>
}

function renderStartupModals() {
  return render(
    <MemoryRouter>
      <StartupModals />
      <LocationProbe />
    </MemoryRouter>
  )
}

describe('StartupModals', () => {
  beforeEach(() => {
    localStorage.clear()
    mockSetupSnapshot.rePairingRequired = false
  })

  afterEach(() => {
    localStorage.clear()
  })

  it('shows the Engine-owned re-pairing notice', async () => {
    mockSetupSnapshot.rePairingRequired = true

    renderStartupModals()

    expect(await screen.findByText('rePairingNotice.title')).toBeVisible()
  })

  it('opens device management and closes the re-pairing notice', async () => {
    mockSetupSnapshot.rePairingRequired = true
    const user = userEvent.setup()
    renderStartupModals()

    await user.click(await screen.findByText('rePairingNotice.goToDevices'))

    expect(screen.getByTestId('location')).toHaveTextContent('/devices')
    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('does not infer re-pairing from a normal startup', () => {
    renderStartupModals()

    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('remembers do not show again across startup remounts without navigating', async () => {
    mockSetupSnapshot.rePairingRequired = true
    const user = userEvent.setup()
    const view = renderStartupModals()

    await user.click(await screen.findByText('rePairingNotice.dontShowAgain'))

    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
    expect(screen.getByTestId('location')).toHaveTextContent(/^\/$/)

    view.unmount()
    renderStartupModals()

    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('still reminds on next startup when only opening devices', async () => {
    mockSetupSnapshot.rePairingRequired = true
    const user = userEvent.setup()
    const view = renderStartupModals()

    await user.click(await screen.findByText('rePairingNotice.goToDevices'))
    view.unmount()
    renderStartupModals()

    expect(await screen.findByText('rePairingNotice.title')).toBeVisible()
  })
})
