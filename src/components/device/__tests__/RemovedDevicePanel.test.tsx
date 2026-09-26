import { render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import type { DeviceTrustRelationship } from '@/api/daemon/device-trust'
import RemovedDevicePanel from '@/components/device/RemovedDevicePanel'
import i18n from '@/i18n'

const device: DeviceTrustRelationship = {
  deviceId: 'removed-peer',
  displayName: 'Office PC',
  isLocal: false,
  reachability: 'offline',
  membership: 'removed',
  groupRelationship: 'awaiting_removal_acknowledgement',
  compatibility: 'compatible',
  syncRelationship: 'removed_peer_device',
  availableActions: [],
  blockedReason: null,
}

describe('RemovedDevicePanel', () => {
  let initialLanguage = 'en-US'

  beforeAll(async () => {
    initialLanguage = i18n.language
    await i18n.changeLanguage('en-US')
  })

  afterAll(async () => {
    await i18n.changeLanguage(initialLanguage)
  })

  it('shows delivery progress without a retry or removal action', () => {
    render(<RemovedDevicePanel device={device} />, {
      wrapper: ({ children }) => <I18nextProvider i18n={i18n}>{children}</I18nextProvider>,
    })

    expect(screen.getByRole('heading', { name: 'Office PC' })).toBeInTheDocument()
    expect(screen.getByText('Removed from this device')).toBeInTheDocument()
    expect(
      screen.getByText('Other devices are being notified. No action is needed.')
    ).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /retry|remove|unpair/i })).not.toBeInTheDocument()
  })
})
