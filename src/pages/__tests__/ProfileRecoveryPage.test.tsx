import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { exportStartupLogs } from '@/api/startup-support'
import i18n from '@/i18n'
import ProfileRecoveryPage from '@/pages/ProfileRecoveryPage'

vi.mock('@/api/startup-support', () => ({ exportStartupLogs: vi.fn() }))

describe('ProfileRecoveryPage', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('zh-CN')
  })
  afterEach(cleanup)

  it('shows classified guidance without destructive or fake repair actions', async () => {
    vi.mocked(exportStartupLogs).mockResolvedValue('/downloads/support.zip')
    render(
      <ProfileRecoveryPage
        status={{
          state: 'admission_recovery_required',
          canSubmitPassphrase: false,
          restartRequired: false,
          backgroundReady: false,
          cleanupPending: false,
          losses: [],
          admission: {
            category: 'legacy_fallback_invalid',
            stage: 'legacy_repository',
            action: 'choose_backup',
          },
        }}
      />
    )

    expect(screen.getByRole('heading')).toHaveTextContent('现有资料无法安全打开')
    expect(screen.getByText('现有资料没有被删除或替换。')).toBeVisible()
    expect(screen.getByText('较早的成员资料也无效')).toBeVisible()
    expect(screen.getByText('使用这个资料档案的可靠备份')).toBeVisible()
    expect(screen.queryByRole('button', { name: /删除|清空|修复/ })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: '导出诊断资料' }))
    await waitFor(() => expect(exportStartupLogs).toHaveBeenCalledOnce())
    expect(await screen.findByRole('status')).toHaveTextContent('诊断资料已导出')
  })
})
