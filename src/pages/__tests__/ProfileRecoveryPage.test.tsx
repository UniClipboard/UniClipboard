import { fireEvent, render, screen, waitFor, cleanup } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { exportStartupLogs } from '@/api/startup-support'
import i18n from '@/i18n'
import { commands } from '@/lib/ipc'
import type { ProfileRecoveryResponse } from '@/lib/ipc-bindings.generated'
import ProfileRecoveryPage from '@/pages/ProfileRecoveryPage'

vi.mock('@/lib/ipc', () => ({ commands: { unlockContent: vi.fn() } }))
vi.mock('@/api/startup-support', () => ({ exportStartupLogs: vi.fn() }))
vi.mock('@/store/setupRealtimeStore', () => ({
  ensureSetupRealtimeSync: vi.fn().mockResolvedValue(undefined),
  refreshSetupState: vi.fn().mockResolvedValue(undefined),
}))
const status: ProfileRecoveryResponse = {
  state: 'awaiting_passphrase',
  canSubmitPassphrase: true,
  restartRequired: false,
  backgroundReady: false,
  cleanupPending: false,
  losses: [],
  admission: null,
}
describe('ProfileRecoveryPage', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('zh-CN')
  })
  afterEach(cleanup)
  it('keeps data inaccessible after a wrong passphrase, then accepts the original untrimmed passphrase', async () => {
    const recovered = vi.fn()
    vi.mocked(commands.unlockContent)
      .mockRejectedValueOnce({ code: 'WRONG_PASSPHRASE' })
      .mockResolvedValueOnce(null)
    render(<ProfileRecoveryPage status={status} onRecovered={recovered} onRestart={vi.fn()} />)
    const input = screen.getByLabelText(i18n.t('unlock.passphraseModal.passphraseLabel'))
    fireEvent.change(input, { target: { value: 'wrong' } })
    fireEvent.click(screen.getByRole('button', { name: i18n.t('profileRecovery.submit') }))
    await screen.findByRole('alert')
    expect(recovered).not.toHaveBeenCalled()
    expect(screen.queryByText(i18n.t('unlock.factoryReset.link'))).toBeNull()
    fireEvent.change(input, { target: { value: ' original with spaces ' } })
    fireEvent.click(screen.getByRole('button', { name: i18n.t('profileRecovery.submit') }))
    await waitFor(() => expect(recovered).toHaveBeenCalledOnce())
    expect(commands.unlockContent).toHaveBeenLastCalledWith({
      passphrase: ' original with spaces ',
    })
    expect(input).toHaveValue('')
  })
  it.each([
    ['PROFILE_RECOVERY_PARTIAL', 'partial'],
    ['PROFILE_RECOVERY_UNSUPPORTED', 'unsupported'],
    ['PROFILE_RECOVERY_PERSISTENCE_FAILED', 'persistenceFailed'],
    ['CORRUPTED_KEY_MATERIAL', 'corrupt'],
  ])('shows safe guidance for %s without exposing server text', async (code, key) => {
    vi.mocked(commands.unlockContent).mockRejectedValue({ code, message: 'secret internal detail' })
    render(<ProfileRecoveryPage status={status} onRecovered={vi.fn()} onRestart={vi.fn()} />)
    fireEvent.change(screen.getByLabelText(i18n.t('unlock.passphraseModal.passphraseLabel')), {
      target: { value: 'test' },
    })
    fireEvent.click(screen.getByRole('button', { name: i18n.t('profileRecovery.submit') }))
    expect(await screen.findByRole('alert')).toHaveTextContent(i18n.t(`profileRecovery.${key}`))
    expect(screen.queryByText('secret internal detail')).toBeNull()
  })
  it('disables submission while Engine is recovering', () => {
    render(
      <ProfileRecoveryPage
        status={{ ...status, state: 'recovering' }}
        onRecovered={vi.fn()}
        onRestart={vi.fn()}
      />
    )
    expect(screen.getByRole('button')).toBeDisabled()
  })
  it.each([
    ['failed', 'failed'],
    ['partially_recoverable', 'partial'],
  ] as const)(
    'explains an existing %s state without requiring another submission',
    (state, key) => {
      render(
        <ProfileRecoveryPage
          status={{ ...status, state, canSubmitPassphrase: false }}
          onRecovered={vi.fn()}
          onRestart={vi.fn()}
        />
      )
      expect(screen.getByRole('alert')).toHaveTextContent(i18n.t(`profileRecovery.${key}`))
      expect(screen.getByRole('button')).toBeDisabled()
    }
  )
  it('requires a background restart after startup failure, even before a local submission', () => {
    const restart = vi.fn()
    render(
      <ProfileRecoveryPage
        status={{ ...status, state: 'failed', canSubmitPassphrase: false, restartRequired: true }}
        onRecovered={vi.fn()}
        onRestart={restart}
      />
    )
    expect(screen.getByRole('alert')).toHaveTextContent(i18n.t('profileRecovery.restartRequired'))
    expect(screen.queryByRole('button', { name: i18n.t('profileRecovery.submit') })).toBeNull()
    expect(screen.getByLabelText(i18n.t('unlock.passphraseModal.passphraseLabel'))).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('profileRecovery.restart') }))
    expect(restart).toHaveBeenCalledOnce()
    expect(commands.unlockContent).not.toHaveBeenCalled()
    expect(screen.queryByText(i18n.t('profileRecovery.recovering'))).toBeNull()
  })

  it('shows stable admission recovery guidance and exports diagnostics', async () => {
    vi.mocked(exportStartupLogs).mockResolvedValue('/downloads/uniclipboard-diagnostics.zip')
    render(
      <ProfileRecoveryPage
        status={{
          ...status,
          state: 'admission_recovery_required',
          canSubmitPassphrase: false,
          admission: {
            category: 'legacy_fallback_invalid',
            stage: 'legacy_repository',
            action: 'choose_backup',
          },
        }}
        onRecovered={vi.fn()}
        onRestart={vi.fn()}
      />
    )

    expect(screen.getByRole('heading')).toHaveTextContent(i18n.t('profileAdmissionRecovery.title'))
    expect(screen.getByText(i18n.t('profileAdmissionRecovery.preserved'))).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('profileAdmissionRecovery.export') }))
    expect(await screen.findByRole('status')).toHaveTextContent(
      i18n.t('profileAdmissionRecovery.exported')
    )
    expect(exportStartupLogs).toHaveBeenCalledOnce()
    expect(commands.unlockContent).not.toHaveBeenCalled()
  })
})
