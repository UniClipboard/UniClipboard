import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import SecuritySection from '@/components/setting/SecuritySection'
import i18n from '@/i18n'

const useDeviceTrust = vi.fn()

vi.mock('@/hooks/useDeviceTrust', () => ({
  useDeviceTrust: () => useDeviceTrust(),
}))

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    setting: { security: { autoUnlockEnabled: true } },
    error: null,
    updateSecuritySetting: vi.fn(),
  }),
}))

describe('SecuritySection passphrase reset', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en-US')
  })

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('disables reset when the space contains another device', () => {
    useDeviceTrust.mockReturnValue({
      loading: false,
      snapshot: {
        localDeviceId: 'local',
        localMembership: 'active',
        devices: [
          { deviceId: 'local', membership: 'active' },
          { deviceId: 'peer', membership: 'active' },
        ],
      },
    })

    render(
      <I18nextProvider i18n={i18n}>
        <SecuritySection />
      </I18nextProvider>
    )

    expect(screen.getByRole('button', { name: 'Reset' })).toBeDisabled()
    expect(
      screen.getByText('Remove the other devices from this space before resetting the passphrase.')
    ).toBeVisible()
  })

  it('opens reset for a single-device space', () => {
    useDeviceTrust.mockReturnValue({
      loading: false,
      snapshot: {
        localDeviceId: 'local',
        localMembership: 'active',
        devices: [{ deviceId: 'local', membership: 'active' }],
      },
    })

    render(
      <I18nextProvider i18n={i18n}>
        <SecuritySection />
      </I18nextProvider>
    )

    fireEvent.click(screen.getByRole('button', { name: 'Reset' }))
    expect(screen.getByRole('heading', { name: 'Reset space passphrase' })).toBeVisible()
  })
})
