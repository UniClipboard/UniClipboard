import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import ChangePassphraseForm from '@/components/security/ChangePassphraseForm'
import i18n from '@/i18n'

const changeEncryptionPassphrase = vi.fn()
const getChangePassphraseErrorCode = vi.fn()
const { logInfo, logWarn } = vi.hoisted(() => ({
  logInfo: vi.fn(),
  logWarn: vi.fn(),
}))

vi.mock('@/api/daemon/encryption', () => ({
  changeEncryptionPassphrase: (passphrase: string, confirmation: string) =>
    changeEncryptionPassphrase(passphrase, confirmation),
  getChangePassphraseErrorCode: (error: unknown) => getChangePassphraseErrorCode(error),
}))

vi.mock('@/lib/logger', () => ({
  createLogger: () => ({ info: logInfo, warn: logWarn }),
}))

describe('ChangePassphraseForm', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en-US')
  })

  beforeEach(() => {
    vi.clearAllMocks()
    changeEncryptionPassphrase.mockResolvedValue(undefined)
    getChangePassphraseErrorCode.mockReturnValue(null)
  })

  it('changes the passphrase after matching confirmation', async () => {
    const onChanged = vi.fn()
    render(
      <I18nextProvider i18n={i18n}>
        <ChangePassphraseForm onCancel={() => undefined} onChanged={onChanged} />
      </I18nextProvider>
    )

    fireEvent.change(screen.getByLabelText('New space passphrase'), {
      target: { value: 'new private phrase' },
    })
    fireEvent.change(screen.getByLabelText('Confirm new passphrase'), {
      target: { value: 'new private phrase' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reset passphrase' }))

    await waitFor(() => expect(onChanged).toHaveBeenCalledOnce())
    expect(changeEncryptionPassphrase).toHaveBeenCalledWith(
      'new private phrase',
      'new private phrase'
    )
    expect(JSON.stringify([...logInfo.mock.calls, ...logWarn.mock.calls])).not.toContain(
      'new private phrase'
    )
  })

  it('does not submit when the two passphrases differ', () => {
    render(
      <I18nextProvider i18n={i18n}>
        <ChangePassphraseForm onCancel={() => undefined} onChanged={() => undefined} />
      </I18nextProvider>
    )

    fireEvent.change(screen.getByLabelText('New space passphrase'), {
      target: { value: 'first phrase' },
    })
    fireEvent.change(screen.getByLabelText('Confirm new passphrase'), {
      target: { value: 'second phrase' },
    })

    expect(screen.getByText('The two passphrases do not match.')).toBeVisible()
    expect(screen.getByRole('button', { name: 'Reset passphrase' })).toBeDisabled()
    expect(changeEncryptionPassphrase).not.toHaveBeenCalled()
  })

  it('explains when another device appeared before submission', async () => {
    changeEncryptionPassphrase.mockRejectedValue(new Error('rejected'))
    getChangePassphraseErrorCode.mockReturnValue('MULTIPLE_DEVICES')
    render(
      <I18nextProvider i18n={i18n}>
        <ChangePassphraseForm onCancel={() => undefined} onChanged={() => undefined} />
      </I18nextProvider>
    )

    fireEvent.change(screen.getByLabelText('New space passphrase'), {
      target: { value: 'same phrase' },
    })
    fireEvent.change(screen.getByLabelText('Confirm new passphrase'), {
      target: { value: 'same phrase' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Reset passphrase' }))

    expect(
      await screen.findByText('Remove the other devices before resetting the passphrase.')
    ).toBeVisible()
  })
})
