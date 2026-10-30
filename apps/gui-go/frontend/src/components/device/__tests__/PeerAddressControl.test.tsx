import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { DaemonApiError, DaemonErrorCode } from '@/api/daemon/errors'
import { PeerAddressControl } from '@/components/device/PeerAddressControl'
import i18n from '@/i18n'
import { DevicesKitPeerAddress } from '@/pages/devices-kit/DevicesKitPeerAddress'
import type { ButtonProps, FormFieldProps, InlineNoticeProps } from '@/ui-kit/contract'

const mocks = vi.hoisted(() => ({ update: vi.fn() }))
vi.mock('@/hooks/useSetting', () => ({
  useSettingSelector: (selector: (context: unknown) => unknown) =>
    selector({
      setting: { network: { peerAddresses: { peer: { ip: '10.10.0.2', port: 51820 } } } },
      loading: false,
      error: null,
      updateNetworkSetting: mocks.update,
    }),
}))
vi.mock('@/ui-kit', () => ({
  SettingsGroup: ({ title, children }: { title: string; children: ReactNode }) => (
    <section aria-label={title}>{children}</section>
  ),
  FormField: ({ label, value, onValueChange, disabled, hint, error }: FormFieldProps) => (
    <label>
      {label}
      <input
        aria-label={label}
        value={value}
        onChange={event => onValueChange(event.target.value)}
        disabled={disabled}
      />
      {hint && <span>{hint}</span>}
      {error && <span>{error}</span>}
    </label>
  ),
  Button: ({ children, disabled, busy, onClick, submitForm }: ButtonProps) => (
    <button
      disabled={disabled || busy}
      onClick={onClick}
      type={submitForm ? 'submit' : 'button'}
      form={submitForm}
    >
      {children}
    </button>
  ),
  InlineNotice: ({ title }: InlineNoticeProps) => <p role="alert">{title}</p>,
}))

for (const [name, Control] of [
  ['legacy', PeerAddressControl],
  ['kit', DevicesKitPeerAddress],
] as const) {
  describe(`${name} direct address control`, () => {
    beforeEach(async () => {
      await i18n.changeLanguage('en-US')
      mocks.update.mockReset().mockResolvedValue({ restartRequired: false })
    })
    const show = () =>
      render(
        <I18nextProvider i18n={i18n}>
          <Control deviceId="peer" />
        </I18nextProvider>
      )
    it('shows stored fields and validates before saving one device', async () => {
      show()
      expect(screen.getByLabelText('IP address')).toHaveValue('10.10.0.2')
      fireEvent.change(screen.getByLabelText('IP address'), { target: { value: 'example.com' } })
      expect(screen.getByText(i18n.t('devices.directAddress.invalidIp'))).toBeVisible()
      expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
      fireEvent.change(screen.getByLabelText('IP address'), { target: { value: '[fd00::2]' } })
      fireEvent.change(screen.getByLabelText('Port'), { target: { value: '0' } })
      expect(screen.getByText(i18n.t('devices.directAddress.invalidPort'))).toBeVisible()
      fireEvent.change(screen.getByLabelText('Port'), { target: { value: '65535' } })
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      await waitFor(() =>
        expect(mocks.update).toHaveBeenCalledWith({
          peerAddresses: { peer: { ip: 'fd00::2', port: 65535 } },
        })
      )
      await waitFor(() => expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled())
    })
    it('clears only this device with null', async () => {
      show()
      fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
      await waitFor(() =>
        expect(mocks.update).toHaveBeenCalledWith({ peerAddresses: { peer: null } })
      )
      await waitFor(() => expect(screen.getByRole('button', { name: 'Clear' })).toBeEnabled())
    })
    it('displays a daemon rejection reason and permits retry', async () => {
      const reason = 'invalid peer address: the device id is not valid'
      mocks.update.mockRejectedValueOnce(
        new DaemonApiError(DaemonErrorCode.INTERNAL_ERROR, 'Bad request', undefined, 400, {
          message: reason,
        })
      )
      show()
      fireEvent.change(screen.getByLabelText('Port'), { target: { value: '5000' } })
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      expect(await screen.findByRole('alert')).toHaveTextContent(reason)
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(2))
      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument())
    })
  })
}
