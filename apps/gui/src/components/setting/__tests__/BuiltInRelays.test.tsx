import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { getRelayOverview } from '@/api/daemon'
import NetworkSection from '@/components/setting/NetworkSection'
import { useSetting } from '@/hooks/useSetting'
import i18n from '@/i18n'
import { makeBaseSettings } from '@/test/fixtures/settings'
import type {
  CustomRelay,
  RelayOverview,
  RelayOverviewEntry,
  SettingContextType,
} from '@/types/setting'

vi.mock('@/api/daemon', () => ({
  CustomRelayMutationError: class extends Error {},
  getRelayOverview: vi.fn(),
}))
vi.mock('@/api/daemon/settings', () => ({ probeRelayUrl: vi.fn() }))
vi.mock('@/lib/ipc', () => ({ commands: { restartDaemon: vi.fn() } }))
vi.mock('@/hooks/useSetting', () => ({ useSetting: vi.fn() }))

const mockOverview = vi.mocked(getRelayOverview)
const mockUseSetting = vi.mocked(useSetting)

const REGIONS: Array<[string, string]> = [
  ['na-east', 'https://use1-1.relay.n0.iroh.link./'],
  ['na-west', 'https://usw1-1.relay.n0.iroh.link./'],
  ['eu', 'https://euc1-1.relay.n0.iroh.link./'],
  ['asia-pacific', 'https://aps1-1.relay.n0.iroh.link./'],
]

const builtIn = (inEffect: boolean): RelayOverviewEntry[] =>
  REGIONS.map(([regionId, url]) => ({
    source: 'builtIn',
    regionId,
    url,
    credentialConfigured: false,
    inEffect,
  }))

const overview = (patch: Partial<RelayOverview> = {}): RelayOverview => ({
  savedMode: 'builtIn',
  appliedMode: 'builtIn',
  changePending: false,
  entries: builtIn(true),
  ...patch,
})

const mountWith = (customRelays: CustomRelay[], relayLoading = false) => {
  mockUseSetting.mockReturnValue({
    setting: makeBaseSettings(),
    loading: false,
    error: null,
    customRelays,
    relayLoading,
    relayError: null,
    reloadSetting: vi.fn(),
    reloadCustomRelays: vi.fn(),
    updateSetting: vi.fn(),
    updateGeneralSetting: vi.fn(),
    updateAutostart: vi.fn(),
    updateSyncSetting: vi.fn(),
    updateSecuritySetting: vi.fn(),
    updateRetentionPolicy: vi.fn(),
    updateKeyboardShortcuts: vi.fn(),
    updateFileSyncSetting: vi.fn(),
    updateNetworkSetting: vi.fn(),
    mutateCustomRelay: vi.fn(),
    updateQuickPanelSetting: vi.fn(),
  } as unknown as SettingContextType)
}

beforeEach(async () => {
  vi.clearAllMocks()
  await i18n.changeLanguage('en-US')
})
afterEach(cleanup)

describe('NetworkSection built-in relays', () => {
  it('lists built-in relays read-only without claiming a connection', async () => {
    mockOverview.mockResolvedValue(overview())
    mountWith([])
    render(<NetworkSection />)

    const list = await screen.findByTestId('built-in-relay-list')
    expect(within(list).getAllByTestId('built-in-relay-row')).toHaveLength(4)
    expect(within(list).getByText('Europe')).toBeInTheDocument()
    expect(within(list).getByText('https://aps1-1.relay.n0.iroh.link./')).toBeInTheDocument()
    expect(within(list).queryByRole('button')).toBeNull()
    expect(within(list).queryByRole('textbox')).toBeNull()
    expect(screen.queryByText(/connected$/i)).toBeNull()
  })

  it('does not query before settings finish loading, then queries once', async () => {
    mockOverview.mockResolvedValue(overview())
    mountWith([], true)
    const view = render(<NetworkSection />)
    expect(mockOverview).not.toHaveBeenCalled()

    mountWith([], false)
    view.rerender(<NetworkSection />)
    await screen.findByTestId('built-in-relay-list')
    expect(mockOverview).toHaveBeenCalledTimes(1)
  })

  it('shows the Engine state after a custom relay change and ignores a stale response', async () => {
    let releaseSlow: (value: RelayOverview) => void = () => {}
    mockOverview
      .mockResolvedValueOnce(overview())
      .mockImplementationOnce(() => new Promise(resolve => (releaseSlow = resolve)))
      .mockResolvedValueOnce(
        overview({ savedMode: 'custom', changePending: true, entries: builtIn(true) })
      )
    mountWith([])
    const view = render(<NetworkSection />)
    await screen.findByTestId('built-in-relay-list')

    // Two quick relay-list changes: the slower first request resolves last and must lose.
    mountWith([{ url: 'https://a.example.com/', credentialConfigured: false }])
    view.rerender(<NetworkSection />)
    mountWith([{ url: 'https://b.example.com/', credentialConfigured: false }])
    view.rerender(<NetworkSection />)
    await waitFor(() =>
      expect(screen.getByTestId('relay-overview-routing')).toHaveAttribute(
        'data-routing-mode',
        'custom'
      )
    )
    await act(async () => releaseSlow(overview()))
    expect(screen.getByTestId('relay-overview-routing')).toHaveAttribute(
      'data-routing-mode',
      'custom'
    )
    expect(screen.getByTestId('relay-overview-applied')).toHaveAttribute(
      'data-change-pending',
      'true'
    )
    expect(screen.getAllByText(/Configured on the running node/).length).toBeGreaterThan(0)
  })

  it('shows a retryable error instead of an empty list, and falls back to the URL for unknown regions', async () => {
    const user = userEvent.setup()
    mockOverview.mockRejectedValueOnce(new Error('boom')).mockResolvedValueOnce(
      overview({
        entries: [
          { ...builtIn(true)[0], regionId: 'sa-east', url: 'https://sae1-1.relay.example.net./' },
        ],
      })
    )
    mountWith([])
    render(<NetworkSection />)

    await screen.findByText('Could not load the built-in relay list.')
    expect(screen.queryByTestId('built-in-relay-row')).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Retry' }))
    const row = await screen.findByTestId('built-in-relay-row')
    expect(within(row).getAllByText('https://sae1-1.relay.example.net./').length).toBeGreaterThan(0)
  })
})
