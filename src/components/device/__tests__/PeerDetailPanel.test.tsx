import { render, screen, waitFor, within } from '@testing-library/react'
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
  onMarkLost: vi.fn(),
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
    memberSyncPreferences: { 'peer-1': preferences },
    memberSyncPreferencesLoading: { 'peer-1': false },
    spaceProtection: null as null | {
      members: Array<{ deviceId: string; status: MemberProtectionStatus }>
    },
    memberRemoval: null as null | {
      outcome: string
      pendingRecipientDeviceIds: string[]
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
    <I18nextProvider i18n={i18n}>
      <PeerDetailPanel
        deviceId="peer-1"
        device={device}
        globalAutoSyncOff={false}
        globalFileSyncOff={false}
        lanOnlyActive={false}
        onUnpair={vi.fn()}
        onMarkLost={mocks.onMarkLost}
      />
    </I18nextProvider>
  )
}

describe('PeerDetailPanel receive controls', () => {
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
    mocks.onMarkLost.mockReset()
    mocks.dispatch.mockReturnValue({ unwrap: () => Promise.resolve(preferences) })
    state.devices.spaceProtection = null
    state.devices.memberRemoval = null
  })

  it('shows send and receive settings in one content-type table', () => {
    renderPanel()

    const table = screen.getByRole('table', { name: 'Sync Settings' })
    expect(within(table).getByRole('columnheader', { name: /Content types/ })).toBeInTheDocument()
    expect(within(table).getByRole('columnheader', { name: /Send/ })).toBeInTheDocument()
    expect(within(table).getByRole('columnheader', { name: /Receive/ })).toBeInTheDocument()

    const textRow = within(table).getByRole('row', { name: /Text/ })
    expect(within(textRow).getByRole('switch', { name: 'Send: Text' })).toBeInTheDocument()
    expect(within(textRow).getByRole('switch', { name: 'Receive: Text' })).toBeInTheDocument()
    expect(screen.queryByRole('tab')).not.toBeInTheDocument()
  })

  it('exposes an accessible sync settings help icon', () => {
    renderPanel()

    expect(screen.getByRole('button', { name: 'About send and receive' })).toBeInTheDocument()
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

  it('updates only the receive master switch when the user disables receiving', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('switch', { name: 'Receive from this device' }))

    await waitFor(() => {
      expect(mocks.updateMemberSyncPreferences).toHaveBeenCalledWith({
        deviceId: 'peer-1',
        patch: { receiveEnabled: false },
      })
    })
  })

  it('updates only one receive content type from the receive section', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByRole('switch', { name: 'Receive: Text' }))

    await waitFor(() => {
      expect(mocks.updateMemberSyncPreferences).toHaveBeenCalledWith({
        deviceId: 'peer-1',
        patch: { receiveContentTypes: { text: false } },
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

    await user.click(screen.getByRole('switch', { name: 'Receive from this device' }))

    await waitFor(() => {
      expect(mocks.toastError).toHaveBeenCalledWith("Couldn't update sync settings. Try again.")
      expect(mocks.fetchMemberSyncPreferences).toHaveBeenCalledTimes(2)
    })
  })

  it('offers a mark-as-lost action while the device awaits the security update', async () => {
    state.devices.memberRemoval = {
      outcome: 'applied',
      pendingRecipientDeviceIds: ['peer-1'],
    }
    const user = userEvent.setup()
    renderPanel()

    await user.click(
      screen.getByRole('button', { name: i18n.t('devices.memberRemoval.permanentLoss.markLost') })
    )

    expect(mocks.onMarkLost).toHaveBeenCalledWith('peer-1')
  })

  it('hides the mark-as-lost action once the removal completes', () => {
    state.devices.memberRemoval = {
      outcome: 'complete',
      pendingRecipientDeviceIds: ['peer-1'],
    }
    renderPanel()

    expect(
      screen.queryByRole('button', {
        name: i18n.t('devices.memberRemoval.permanentLoss.markLost'),
      })
    ).not.toBeInTheDocument()
  })
})
