import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import ResetSpaceDangerZone from '@/components/device/ResetSpaceDangerZone'
import i18n from '@/i18n'

const resetSetup = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())

vi.mock('@/api/daemon/setupV2', () => ({ resetSetup }))
vi.mock('@/components/ui/toast', () => ({
  toast: { success: toastSuccess, error: vi.fn() },
}))

describe('ResetSpaceDangerZone', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    await i18n.changeLanguage('en-US')
    resetSetup.mockResolvedValue(undefined)
  })

  it('explains the irreversible effects and resets only after confirmation', async () => {
    const user = userEvent.setup()
    const onResetComplete = vi.fn()
    render(
      <I18nextProvider i18n={i18n}>
        <ResetSpaceDangerZone onResetComplete={onResetComplete} />
      </I18nextProvider>
    )

    await user.click(screen.getByRole('button', { name: 'Reset Space' }))

    expect(resetSetup).not.toHaveBeenCalled()
    expect(screen.getByText(/Other devices will not be erased/)).toBeVisible()
    expect(screen.getByText(/Every device must be paired again/)).toBeVisible()
    expect(screen.getByText(/Clipboard history, completed files/)).toBeVisible()

    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Reset Space' })
    )

    await waitFor(() => expect(resetSetup).toHaveBeenCalledOnce())
    expect(onResetComplete).toHaveBeenCalledOnce()
    expect(toastSuccess).toHaveBeenCalledOnce()
  })
})
