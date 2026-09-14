import { act, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import ChangePassphraseDialog from '@/components/security/ChangePassphraseDialog'
import i18n from '@/i18n'

const changeEncryptionPassphrase = vi.fn()

vi.mock('@/api/daemon/encryption', () => ({
  changeEncryptionPassphrase: (passphrase: string, confirmation: string) =>
    changeEncryptionPassphrase(passphrase, confirmation),
  getChangePassphraseErrorCode: () => null,
}))

vi.mock('@/lib/logger', () => ({
  createLogger: () => ({ info: vi.fn(), warn: vi.fn() }),
}))

describe('ChangePassphraseDialog', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en-US')
  })

  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('blocks every dismissal path while the passphrase change is pending', async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    let finish!: () => void
    changeEncryptionPassphrase.mockImplementation(
      () =>
        new Promise<void>(resolve => {
          finish = resolve
        })
    )

    render(
      <I18nextProvider i18n={i18n}>
        <ChangePassphraseDialog open onOpenChange={onOpenChange} onChanged={() => undefined} />
      </I18nextProvider>
    )

    await user.type(screen.getByLabelText('New space passphrase'), 'new private phrase')
    await user.type(screen.getByLabelText('Confirm new passphrase'), 'new private phrase')
    await user.click(screen.getByRole('button', { name: 'Reset passphrase' }))

    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(onOpenChange).not.toHaveBeenCalled()

    await act(async () => finish())
    expect(screen.getByRole('button', { name: 'Close' })).toBeVisible()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(onOpenChange).toHaveBeenCalledWith(false)
  })
})
