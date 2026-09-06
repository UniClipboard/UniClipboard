import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { expect, it, vi } from 'vitest'
import DeviceListItem from '@/components/device/DeviceListItem'

it('selects devices with arrow keys without moving focus into the settings', async () => {
  const user = userEvent.setup()
  const select = vi.fn()
  render(
    <div data-device-list>
      <DeviceListItem
        name="My Mac"
        selected
        tone="success"
        status={{ kind: 'online', label: 'Sync enabled' }}
        onSelect={vi.fn()}
      />
      <DeviceListItem
        name="Office PC"
        selected={false}
        tone="off"
        status={{ kind: 'offline', label: 'Offline' }}
        onSelect={select}
      />
    </div>
  )
  screen.getByRole('button', { name: 'My Mac Sync enabled' }).focus()
  await user.keyboard('{ArrowDown}')
  expect(screen.getByRole('button', { name: 'Office PC Offline' })).toHaveFocus()
  expect(select).toHaveBeenCalledOnce()
  await user.keyboard('{Home}')
  expect(screen.getByRole('button', { name: 'My Mac Sync enabled' })).toHaveFocus()
})

it('lets the whole status area select the device', async () => {
  const user = userEvent.setup()
  const select = vi.fn()
  render(
    <DeviceListItem
      name="Office PC"
      selected={false}
      tone="warning"
      status={{ kind: 'diverged', label: 'Needs attention' }}
      onSelect={select}
    />
  )
  await user.click(screen.getByText('Needs attention'))
  expect(select).toHaveBeenCalledOnce()
})
