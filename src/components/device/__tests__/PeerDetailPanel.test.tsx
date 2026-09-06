import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import type { MemberProtectionStatus, MemberSyncPreferences } from '@/api/daemon/member'
import type { SpaceMember } from '@/api/daemon/members'
import PeerDetailPanel from '@/components/device/PeerDetailPanel'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  dispatch: vi.fn(),
  fetchMemberSyncPreferences: vi.fn((deviceId: string) => ({
    type: 'devices/fetchMemberSyncPreferences',
    payload: deviceId,
  })),
  updateMemberSyncPreferences: vi.fn(
    (payload: { deviceId: string; patch: Record<string, unknown> }) => ({
      type: 'devices/updateMemberSyncPreferences',
      payload,
    })
  ),
  toastError: vi.fn(),
}))

const preferences: MemberSyncPreferences = {
  sendEnabled: true,
  receiveEnabled: true,
  sendContentTypes: {
    text: true,
    image: true,
    link: true,
    file: true,
    codeSnippet: true,
    richText: true,
  },
  receiveContentTypes: {
    text: true,
    image: true,
    link: true,
    file: true,
    codeSnippet: true,
    richText: true,
  },
}

const state = {
  devices: {
    memberSyncPreferences: { 'peer-1': preferences } as Record<string, MemberSyncPreferences>,
    memberSyncPreferencesLoading: { 'peer-1': false },
    spaceProtection: null as null | {
      members: Array<{ deviceId: string; status: MemberProtectionStatus }>
    },
    memberRemoval: null as null | {
      phase: string
      intentCount: number
    },
  },
}

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => mocks.dispatch,
  useAppSelector: (selector: (value: typeof state) => unknown) => selector(state),
}))

vi.mock('@/store/slices/devicesSlice', () => ({
  fetchMemberSyncPreferences: mocks.fetchMemberSyncPreferences,
  updateMemberSyncPreferences: mocks.updateMemberSyncPreferences,
}))

vi.mock('@/components/ui/toast', () => ({
  toast: { error: mocks.toastError },
}))

const device: SpaceMember = {
  peerId: 'peer-1',
  deviceName: 'Office PC',
  pairingState: 'Trusted',
  lastSeenAtMs: null,
  connected: true,
  channel: 'direct',
  connectionAddress: '192.168.1.2:5000',
}

function renderPanel() {
  return render(
    <PeerDetailPanel
      deviceId="peer-1"
      device={device}
      globalSyncOff={false}
      globalFileSyncOff={false}
      lanOnlyActive={false}
      onUnpair={vi.fn()}
    />,
    { wrapper: ({ children }) => <I18nextProvider i18n={i18n}>{children}</I18nextProvider> }
  )
}

describe('PeerDetailPanel sync controls', () => {
  let initialLanguage = 'en-US'

  beforeAll(async () => {
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  beforeEach(() => {
    mocks.dispatch.mockReset()
    mocks.fetchMemberSyncPreferences.mockClear()
    mocks.updateMemberSyncPreferences.mockClear()
    mocks.toastError.mockReset()
    mocks.dispatch.mockReturnValue({ unwrap: () => Promise.resolve(preferences) })
    state.devices.spaceProtection = null
    state.devices.memberRemoval = null
    state.devices.memberSyncPreferences = { 'peer-1': preferences }
    state.devices.memberSyncPreferencesLoading['peer-1'] = false
  })

  it('keeps saved controls stable while refreshing a reselected device', () => {
    state.devices.memberSyncPreferencesLoading['peer-1'] = true
    renderPanel()
    expect(screen.getByRole('switch', { name: 'Sync with Office PC' })).toBeEnabled()
    expect(screen.getByRole('combobox', { name: 'Sync direction for Text' })).toBeEnabled()
  })

  it('keeps the settings structure mounted throughout the first load', () => {
    state.devices.memberSyncPreferences = {}
    const view = renderPanel()
    const region = screen.getByRole('region', { name: 'Sync Settings' })
    const control = region.querySelector('[data-slot="select-trigger"]')
    expect(region).toHaveAttribute('aria-busy', 'true')
    expect(control).not.toBeNull()
    expect(control).toBeDisabled()

    state.devices.memberSyncPreferencesLoading['peer-1'] = true
    view.rerender(
      <PeerDetailPanel
        deviceId="peer-1"
        device={device}
        globalSyncOff={false}
        globalFileSyncOff={false}
        lanOnlyActive={false}
        onUnpair={vi.fn()}
      />
    )
    expect(
      screen
        .getByRole('region', { name: 'Sync Settings' })
        .querySelector('[data-slot="select-trigger"]')
    ).toBe(control)
    state.devices.memberSyncPreferences['peer-1'] = preferences
    state.devices.memberSyncPreferencesLoading['peer-1'] = false
    view.rerender(
      <PeerDetailPanel
        deviceId="peer-1"
        device={device}
        globalSyncOff={false}
        globalFileSyncOff={false}
        lanOnlyActive={false}
        onUnpair={vi.fn()}
      />
    )
    expect(screen.getByRole('combobox', { name: 'Sync direction for Text' })).toBe(control)
    expect(control).toBeEnabled()
  })

  it('shows the content direction choices without a collapse control', () => {
    renderPanel()

    expect(screen.getByRole('switch', { name: 'Sync with Office PC' })).toBeChecked()
    expect(
      screen.queryByRole('button', { name: /Customize synced content/ })
    ).not.toBeInTheDocument()
    expect(screen.getAllByRole('combobox')).toHaveLength(5)
  })

  it('shows one direction choice for each content type', () => {
    renderPanel()

    expect(screen.getByRole('combobox', { name: 'Sync direction for Text' })).toHaveTextContent(
      'Both directions'
    )
    expect(screen.getAllByRole('combobox')).toHaveLength(5)
  })

  it('explains that a peer awaiting readmission needs its client updated', () => {
    state.devices.spaceProtection = {
      members: [{ deviceId: 'peer-1', status: 'awaiting_readmission' }],
    }

    renderPanel()

    expect(
      screen.getByRole('button', {
        name: 'Update UniClipboard on the other device to the latest version.',
      })
    ).toBeInTheDocument()
  })

  it('turns off both directions from the device sync switch', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('switch', { name: 'Sync with Office PC' }))

    await waitFor(() => {
      expect(mocks.updateMemberSyncPreferences).toHaveBeenCalledWith({
        deviceId: 'peer-1',
        patch: { sendEnabled: false, receiveEnabled: false },
      })
    })
  })

  it('maps one content direction choice to the existing send and receive settings', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('combobox', { name: 'Sync direction for Text' }))
    await user.click(await screen.findByRole('option', { name: 'Only send to Office PC' }))

    await waitFor(() => {
      expect(mocks.updateMemberSyncPreferences).toHaveBeenCalledWith({
        deviceId: 'peer-1',
        patch: {
          sendContentTypes: { text: true },
          receiveContentTypes: { text: false },
        },
      })
    })
  })

  it('restores both send and receive preferences to their defaults', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('button', { name: 'Restore defaults' }))

    await waitFor(() => {
      expect(mocks.updateMemberSyncPreferences).toHaveBeenCalledWith({
        deviceId: 'peer-1',
        patch: {
          sendEnabled: true,
          receiveEnabled: true,
          sendContentTypes: preferences.sendContentTypes,
          receiveContentTypes: preferences.receiveContentTypes,
        },
      })
    })
  })

  it('shows an error and reloads authoritative preferences when an update fails', async () => {
    const user = userEvent.setup()
    mocks.dispatch.mockImplementation((action: { type: string }) => ({
      unwrap: () =>
        action.type === 'devices/updateMemberSyncPreferences'
          ? Promise.reject(new Error('offline'))
          : Promise.resolve(preferences),
    }))
    renderPanel()

    await user.click(screen.getByRole('switch', { name: 'Sync with Office PC' }))

    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledWith("Couldn't update sync settings. Try again.")
      expect(mocks.fetchMemberSyncPreferences).toHaveBeenCalledTimes(2)
    })
  })
})
