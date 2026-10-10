import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import DeviceListFooter from '@/components/device/DeviceListFooter'
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
describe('DeviceListFooter', () => {
  it('shows the mobile connection action directly without a settings button', async () => {
    const user = userEvent.setup()
    const onAddMobile = vi.fn()
    render(
      <DeviceListFooter
        onlineCount={1}
        onAddDevice={vi.fn()}
        onSwitchSpace={vi.fn()}
        onAddMobile={onAddMobile}
      />
    )
    expect(screen.getByRole('button', { name: 'devices.connectMobile.title' })).toBeVisible()
    await user.click(screen.getByRole('button', { name: 'devices.connectMobile.title' }))
    expect(onAddMobile).toHaveBeenCalledTimes(1)
    expect(
      screen.queryByRole('button', { name: 'devices.mobileSync.configure' })
    ).not.toBeInTheDocument()
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })
})
