import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import DeviceListFooter from '@/components/device/DeviceListFooter'
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
describe('DeviceListFooter menu', () => {
  it('uses the shared menu and invokes the selected action', async () => {
    const user = userEvent.setup()
    const onAddMobile = vi.fn()
    render(
      <DeviceListFooter
        onlineCount={1}
        onAddDevice={vi.fn()}
        onSwitchSpace={vi.fn()}
        onAddMobile={onAddMobile}
        onMobileSettings={vi.fn()}
      />
    )
    await user.click(screen.getByRole('button', { name: 'devices.panel.addMenu.otherWays' }))
    expect(screen.getByRole('menu')).toHaveAttribute('data-slot', 'context-menu-content')
    await user.click(screen.getByRole('menuitem', { name: 'devices.panel.addMenu.mobile' }))
    expect(onAddMobile).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })
})
