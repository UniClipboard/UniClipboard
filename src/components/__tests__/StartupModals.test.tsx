/**
 * StartupModals coordinator — priority and queueing tests.
 *
 * The contract under test:
 * 1. When the daemon reports `Upgraded { from: null }`, the upgrade notice
 *    shows first; only after dismissal does the telemetry notice render.
 * 2. When the daemon reports any other status, telemetry runs immediately
 *    (no upgrade notice).
 * 3. When the daemon HTTP fetch fails, the coordinator falls back to
 *    telemetry (don't block the entire app on a daemon hiccup).
 */

import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getUpgradeStatus, type UpgradeStatus } from '@/api/daemon'
import { connectDaemonWs } from '@/lib/daemon-ws-bootstrap'
import StartupModals from '../StartupModals'

const mockUpdateGeneralSetting = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))

vi.mock('@/api/daemon', async () => {
  const actual = await vi.importActual<typeof import('@/api/daemon')>('@/api/daemon')
  return {
    ...actual,
    getUpgradeStatus: vi.fn(),
    acknowledgeUpgrade: vi.fn(),
  }
})

vi.mock('@/lib/daemon-ws-bootstrap', () => ({
  connectDaemonWs: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    updateGeneralSetting: mockUpdateGeneralSetting,
  }),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, string>) =>
      vars ? `${key}:${JSON.stringify(vars)}` : key,
  }),
}))

const mockedGetUpgradeStatus = vi.mocked(getUpgradeStatus)
const mockedConnectDaemonWs = vi.mocked(connectDaemonWs)

describe('StartupModals priority ordering', () => {
  beforeEach(() => {
    localStorage.clear()
    mockedGetUpgradeStatus.mockReset()
    mockedConnectDaemonWs.mockReset()
    mockUpdateGeneralSetting.mockReset()
    mockUpdateGeneralSetting.mockResolvedValue(undefined)
    mockedConnectDaemonWs.mockResolvedValue(undefined)
  })

  afterEach(() => {
    localStorage.clear()
  })

  it('shows the upgrade notice first when status is upgraded-from-null, even if telemetry is also pending', async () => {
    mockedGetUpgradeStatus.mockResolvedValue({
      kind: 'upgraded',
      from: null,
      to: '1.0.0-alpha.1',
    } satisfies UpgradeStatus)

    render(<StartupModals />)

    // Upgrade title should appear; telemetry title must not be in the DOM yet.
    await waitFor(() => {
      expect(screen.getByText('upgradeNotice.title')).toBeInTheDocument()
    })
    expect(
      screen.queryByText('settings.sections.general.telemetry.notice.title')
    ).not.toBeInTheDocument()
  })

  it('skips the upgrade notice when status is not upgraded-from-null', async () => {
    mockedGetUpgradeStatus.mockResolvedValue({
      kind: 'no_change',
      current: '1.0.0-alpha.1',
    } satisfies UpgradeStatus)

    render(<StartupModals />)

    // Telemetry should appear because localStorage has no record.
    await waitFor(() => {
      expect(
        screen.getByText('settings.sections.general.telemetry.notice.title')
      ).toBeInTheDocument()
    })
    expect(screen.queryByText('upgradeNotice.title')).not.toBeInTheDocument()
  })

  it('skips the upgrade notice when from is a known version (the user already saw a previous version)', async () => {
    mockedGetUpgradeStatus.mockResolvedValue({
      kind: 'upgraded',
      from: '1.0.0-alpha.1',
      to: '1.0.1',
    } satisfies UpgradeStatus)

    render(<StartupModals />)

    await waitFor(() => {
      expect(
        screen.getByText('settings.sections.general.telemetry.notice.title')
      ).toBeInTheDocument()
    })
    expect(screen.queryByText('upgradeNotice.title')).not.toBeInTheDocument()
  })

  it('falls back to telemetry when the daemon fetch fails', async () => {
    mockedGetUpgradeStatus.mockRejectedValue(new Error('daemon offline'))

    render(<StartupModals />)

    await waitFor(() => {
      expect(
        screen.getByText('settings.sections.general.telemetry.notice.title')
      ).toBeInTheDocument()
    })
    expect(screen.queryByText('upgradeNotice.title')).not.toBeInTheDocument()
  })

  it('does not show telemetry when localStorage already has the dismissed flag', async () => {
    localStorage.setItem('uc-telemetry-notice-seen', '1')
    mockedGetUpgradeStatus.mockResolvedValue({
      kind: 'no_change',
      current: '1.0.0-alpha.1',
    } satisfies UpgradeStatus)

    render(<StartupModals />)

    // Wait a tick so the effect runs.
    await waitFor(() => {
      expect(mockedGetUpgradeStatus).toHaveBeenCalled()
    })
    expect(
      screen.queryByText('settings.sections.general.telemetry.notice.title')
    ).not.toBeInTheDocument()
    expect(screen.queryByText('upgradeNotice.title')).not.toBeInTheDocument()
  })

  it('disables diagnostics and usage analytics when the user opts out', async () => {
    mockedGetUpgradeStatus.mockResolvedValue({
      kind: 'no_change',
      current: '1.0.0-alpha.1',
    } satisfies UpgradeStatus)

    const user = userEvent.setup()
    render(<StartupModals />)

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
