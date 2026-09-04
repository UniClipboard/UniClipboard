import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, useLocation } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import StartupModals from '@/components/StartupModals'

const mockUpdateGeneralSetting = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
const mockSetupSnapshot = vi.hoisted(() => ({ rePairingRequired: false }))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    updateGeneralSetting: mockUpdateGeneralSetting,
  }),
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
    mockUpdateGeneralSetting.mockReset()
    mockUpdateGeneralSetting.mockResolvedValue(undefined)
  })

  afterEach(() => {
    localStorage.clear()
  })

  it('shows the Engine-owned re-pairing notice before telemetry', async () => {
    mockSetupSnapshot.rePairingRequired = true

    renderStartupModals()

    expect(await screen.findByText('rePairingNotice.title')).toBeVisible()
    expect(screen.getByRole('alertdialog')).toHaveStyle({ animation: 'none' })
    expect(
      screen.queryByText('settings.sections.general.telemetry.notice.title')
    ).not.toBeInTheDocument()
  })

  it('opens device management and closes the re-pairing notice', async () => {
    mockSetupSnapshot.rePairingRequired = true
    const user = userEvent.setup()
    renderStartupModals()

    await user.click(await screen.findByText('rePairingNotice.goToDevices'))

    expect(screen.getByTestId('location')).toHaveTextContent('/devices')
    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('does not infer re-pairing from a normal startup', async () => {
    renderStartupModals()

    expect(
      await screen.findByText('settings.sections.general.telemetry.notice.title')
    ).toBeInTheDocument()
    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('does not show telemetry when it was already dismissed', async () => {
    localStorage.setItem('uc-telemetry-notice-seen', '1')

    renderStartupModals()

    await waitFor(() => {
      expect(
        screen.queryByText('settings.sections.general.telemetry.notice.title')
      ).not.toBeInTheDocument()
    })
    expect(screen.queryByText('rePairingNotice.title')).not.toBeInTheDocument()
  })

  it('disables diagnostics and usage analytics when the user opts out', async () => {
    const user = userEvent.setup()
    renderStartupModals()

    await user.click(await screen.findByText('settings.sections.general.telemetry.notice.optOut'))

    await waitFor(() => {
      expect(mockUpdateGeneralSetting).toHaveBeenCalledWith({
        telemetryEnabled: false,
        usageAnalyticsEnabled: false,
      })
    })
    expect(localStorage.getItem('uc-telemetry-notice-seen')).toBe('1')
  })
})
