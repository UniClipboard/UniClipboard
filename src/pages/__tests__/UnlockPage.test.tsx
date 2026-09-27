import { render, screen, act, fireEvent, waitFor } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { resetSpace, verifyKeychainAccess } from '@/api/security'
import i18n from '@/i18n'
import { commands } from '@/lib/ipc'
import UnlockPage from '@/pages/UnlockPage'
import { refreshSetupState } from '@/store/setupRealtimeStore'

vi.mock('@/api/security', async () => {
  const actual = await vi.importActual<typeof import('@/api/security')>('@/api/security')
  return {
    ...actual,
    unlockSpaceWithPassphrase: vi.fn(),
    verifyKeychainAccess: vi.fn(),
    resetSpace: vi.fn(),
  }
})

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: () => ({ isMac: false }),
}))

vi.mock('@/lib/ipc', () => ({
  commands: { unlockContent: vi.fn(), unlockContentFromKeyring: vi.fn() },
}))

const updateSecuritySettingMock = vi.fn()

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    setting: { security: { autoUnlockEnabled: false } },
    updateSecuritySetting: updateSecuritySettingMock,
    loading: false,
  }),
}))

vi.mock('@/store/setupRealtimeStore', () => ({
  ensureSetupRealtimeSync: vi.fn().mockResolvedValue(undefined),
  refreshSetupState: vi.fn().mockResolvedValue(undefined),
}))

describe('UnlockPage', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('zh-CN')
    vi.mocked(verifyKeychainAccess).mockResolvedValue(true)
    vi.mocked(commands.unlockContentFromKeyring).mockResolvedValue(false)
  })

  it('does not render the lower-right blurred decorative effect', () => {
    const { container } = render(<UnlockPage />)

    expect(container.querySelector('[data-uc-decorative-effect].blur-3xl')).toBeNull()
  })

  it('keeps the main unlock view focused on one primary action', () => {
    const { container } = render(<UnlockPage />)

    expect(screen.queryByText(i18n.t('settings.sections.security.title'))).not.toBeInTheDocument()
    expect(screen.getAllByText('UniClipboard')).toHaveLength(1)
    expect(screen.getByRole('button', { name: i18n.t('unlock.button') })).toHaveClass('w-full')
    expect(container.querySelector('.max-w-md')).not.toBeNull()
  })

  it('unlocks directly when the explicit keyring attempt succeeds', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(commands.unlockContentFromKeyring).mockResolvedValue(true)

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    expect(verifyKeychainAccess).not.toHaveBeenCalled()
    expect(commands.unlockContent).not.toHaveBeenCalled()
    expect(commands.unlockContentFromKeyring).toHaveBeenCalledTimes(1)
    expect(onUnlockSucceeded).toHaveBeenCalledTimes(1)
    expect(
      screen.queryByLabelText(i18n.t('unlock.passphraseModal.passphraseLabel'))
    ).not.toBeInTheDocument()
    expect(screen.queryByRole('switch')).not.toBeInTheDocument()
  })

  it('shows the inline passphrase form when secure storage authentication is unavailable', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(commands.unlockContentFromKeyring).mockResolvedValue(false)

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    expect(onUnlockSucceeded).not.toHaveBeenCalled()
    expect(screen.getByText(i18n.t('unlock.passphraseModal.title'))).toBeInTheDocument()
  })

  it('shows the inline passphrase form when secure storage authentication rejects', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(commands.unlockContentFromKeyring).mockRejectedValue({
      code: 'INTERNAL',
      message: 'silent unlock failed: WrongPassphrase',
    })

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    expect(onUnlockSucceeded).not.toHaveBeenCalled()
    expect(screen.getByText(i18n.t('unlock.passphraseModal.title'))).toBeInTheDocument()
  })

  it('successfully unlocks with a correct passphrase from the inline form', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(verifyKeychainAccess).mockResolvedValue(false)
    vi.mocked(commands.unlockContent).mockResolvedValue(null)

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    const passphraseInput = screen.getByLabelText(
      i18n.t('unlock.passphraseModal.passphraseLabel')
    ) as HTMLInputElement
    fireEvent.change(passphraseInput, {
      target: { value: 'correct-horse-battery-staple' },
    })

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.passphraseModal.submit') }).click()
    })

    expect(commands.unlockContent).toHaveBeenCalledWith({
      passphrase: 'correct-horse-battery-staple',
    })
    expect(refreshSetupState).toHaveBeenCalledTimes(1)
    expect(onUnlockSucceeded).toHaveBeenCalledTimes(1)
  })

  it('shows WRONG_PASSPHRASE message and keeps the form open on wrong passphrase', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(verifyKeychainAccess).mockResolvedValue(false)
    vi.mocked(commands.unlockContent).mockRejectedValue({
      code: 'WRONG_PASSPHRASE',
    })

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    const passphraseInput = screen.getByLabelText(
      i18n.t('unlock.passphraseModal.passphraseLabel')
    ) as HTMLInputElement
    fireEvent.change(passphraseInput, {
      target: { value: 'wrong-passphrase' },
    })

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.passphraseModal.submit') }).click()
    })

    await waitFor(() => {
      expect(screen.getByText(i18n.t('unlock.errors.wrongPassphrase'))).toBeInTheDocument()
    })
    expect(onUnlockSucceeded).not.toHaveBeenCalled()
    // Modal stays open so the user can retry
    expect(screen.getByText(i18n.t('unlock.passphraseModal.title'))).toBeInTheDocument()
  })

  it('shows CORRUPTED_KEY_MATERIAL guidance when the keyslot is broken', async () => {
    const onUnlockSucceeded = vi.fn()
    vi.mocked(verifyKeychainAccess).mockResolvedValue(false)
    vi.mocked(commands.unlockContent).mockRejectedValue({
      code: 'CORRUPTED_KEY_MATERIAL',
    })

    render(<UnlockPage onUnlockSucceeded={onUnlockSucceeded} />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    const passphraseInput = screen.getByLabelText(
      i18n.t('unlock.passphraseModal.passphraseLabel')
    ) as HTMLInputElement
    fireEvent.change(passphraseInput, { target: { value: 'whatever' } })

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.passphraseModal.submit') }).click()
    })

    await waitFor(() => {
      expect(screen.getByText(i18n.t('profileRecovery.corrupt'))).toBeInTheDocument()
    })
  })

  it('clears the error message as soon as the user keeps typing', async () => {
    vi.mocked(verifyKeychainAccess).mockResolvedValue(false)
    vi.mocked(commands.unlockContent).mockRejectedValue({
      code: 'WRONG_PASSPHRASE',
    })

    render(<UnlockPage />)

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.button') }).click()
    })

    const passphraseInput = screen.getByLabelText(
      i18n.t('unlock.passphraseModal.passphraseLabel')
    ) as HTMLInputElement
    fireEvent.change(passphraseInput, { target: { value: 'wrong' } })

    await act(async () => {
      screen.getByRole('button', { name: i18n.t('unlock.passphraseModal.submit') }).click()
    })

    await waitFor(() => {
      expect(screen.getByText(i18n.t('unlock.errors.wrongPassphrase'))).toBeInTheDocument()
    })

    fireEvent.change(passphraseInput, { target: { value: 'wrong-extra' } })

    expect(screen.queryByText(i18n.t('unlock.errors.wrongPassphrase'))).not.toBeInTheDocument()
  })

  describe('factory reset fallback', () => {
    it('requires RESET token before enabling the confirm button', async () => {
      vi.mocked(resetSpace).mockResolvedValue(undefined)
      render(<UnlockPage />)

      // 主页面底部有一个,passphrase modal 里有一个;主页面那个永远可见。
      const resetLink = screen.getAllByRole('button', {
        name: i18n.t('unlock.factoryReset.link'),
      })[0]
      await act(async () => resetLink.click())

      const confirmBtn = screen.getByRole('button', {
        name: i18n.t('unlock.factoryReset.modal.confirm'),
      }) as HTMLButtonElement
      expect(confirmBtn.disabled).toBe(true)

      const input = screen.getByLabelText(
        i18n.t('unlock.factoryReset.modal.confirmPrompt')
      ) as HTMLInputElement
      fireEvent.change(input, { target: { value: 'reset' } })
      expect(confirmBtn.disabled).toBe(true)

      fireEvent.change(input, { target: { value: 'RESET' } })
      expect(confirmBtn.disabled).toBe(false)
    })

    it('calls resetSpace and notifies parent on success', async () => {
      const onResetSucceeded = vi.fn()
      vi.mocked(resetSpace).mockResolvedValue(undefined)
      render(<UnlockPage onResetSucceeded={onResetSucceeded} />)

      const resetLink = screen.getAllByRole('button', {
        name: i18n.t('unlock.factoryReset.link'),
      })[0]
      await act(async () => resetLink.click())

      const input = screen.getByLabelText(
        i18n.t('unlock.factoryReset.modal.confirmPrompt')
      ) as HTMLInputElement
      fireEvent.change(input, { target: { value: 'RESET' } })

      await act(async () => {
        screen
          .getByRole('button', {
            name: i18n.t('unlock.factoryReset.modal.confirm'),
          })
          .click()
      })

      expect(resetSpace).toHaveBeenCalledTimes(1)
      expect(onResetSucceeded).toHaveBeenCalledTimes(1)
      await waitFor(() => {
        expect(
          screen.queryByText(i18n.t('unlock.factoryReset.modal.title'))
        ).not.toBeInTheDocument()
      })
    })

    it('shows typed error and keeps modal open when key wipe fails', async () => {
      const onResetSucceeded = vi.fn()
      vi.mocked(resetSpace).mockRejectedValue({
        code: 'KEY_MATERIAL_WIPE_FAILED',
        message: 'disk i/o',
      })
      render(<UnlockPage onResetSucceeded={onResetSucceeded} />)

      const resetLink = screen.getAllByRole('button', {
        name: i18n.t('unlock.factoryReset.link'),
      })[0]
      await act(async () => resetLink.click())

      const input = screen.getByLabelText(
        i18n.t('unlock.factoryReset.modal.confirmPrompt')
      ) as HTMLInputElement
      fireEvent.change(input, { target: { value: 'RESET' } })

      await act(async () => {
        screen
          .getByRole('button', {
            name: i18n.t('unlock.factoryReset.modal.confirm'),
          })
          .click()
      })

      await waitFor(() => {
        expect(
          screen.getByText(i18n.t('unlock.factoryReset.errors.keyMaterialWipeFailed'))
        ).toBeInTheDocument()
      })
      expect(onResetSucceeded).not.toHaveBeenCalled()
      // Modal stays open so the user can decide what to do next
      expect(screen.getByText(i18n.t('unlock.factoryReset.modal.title'))).toBeInTheDocument()
    })
  })
})
