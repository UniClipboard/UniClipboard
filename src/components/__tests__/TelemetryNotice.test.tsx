import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TelemetryNotice from '@/components/TelemetryNotice'
import { useSetting } from '@/hooks/useSetting'

vi.mock('@/hooks/useSetting', () => ({ useSetting: vi.fn() }))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

const mockUseSetting = vi.mocked(useSetting)

describe('TelemetryNotice', () => {
  beforeEach(() => {
    localStorage.clear()
    mockUseSetting.mockReturnValue({
      updateGeneralSetting: vi.fn().mockResolvedValue(undefined),
    } as unknown as ReturnType<typeof useSetting>)
  })

  it('derives visibility from the coordinator gate without an effect pass', () => {
    const { rerender } = render(<TelemetryNotice enabled={false} />)
    expect(screen.queryByRole('alertdialog')).toBeNull()

    rerender(<TelemetryNotice enabled />)
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()

    rerender(<TelemetryNotice enabled={false} />)
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it('persists the user decision and notifies the startup coordinator', async () => {
    const onDismiss = vi.fn()
    const user = userEvent.setup()
    render(<TelemetryNotice enabled onDismiss={onDismiss} />)

    await user.click(
      screen.getByRole('button', { name: 'settings.sections.general.telemetry.notice.accept' })
    )

    expect(localStorage.getItem('uc-telemetry-notice-seen')).toBe('1')
    expect(onDismiss).toHaveBeenCalledOnce()
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })
})
