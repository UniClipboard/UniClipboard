import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import ClipboardSendMenu from '@/components/clipboard/ClipboardSendMenu'
const mocks = vi.hoisted(() => ({ all: vi.fn(), peer: vi.fn() }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
vi.mock('@/store/hooks', () => ({
  useAppSelector: (
    select: (state: {
      devices: { spaceMembers: { peerId: string; deviceName: string; connected: boolean }[] }
    }) => unknown
  ) =>
    select({
      devices: {
        spaceMembers: [
          { peerId: 'online', deviceName: 'Laptop', connected: true },
          { peerId: 'offline', deviceName: 'Phone', connected: false },
        ],
      },
    }),
}))
vi.mock('@/hooks/useResendAction', () => ({
  useResendAction: () => ({
    resendAll: mocks.all,
    resendToPeer: mocks.peer,
    isEntryInFlight: () => false,
    isPeerInFlight: () => false,
  }),
}))
describe('ClipboardSendMenu', () => {
  it('sends only to the chosen device and disables offline devices', async () => {
    const user = userEvent.setup()
    render(<ClipboardSendMenu entryId="entry" />)
    await user.click(screen.getByRole('button', { name: 'clipboard.contextMenu.send' }))
    expect(screen.getByRole('menuitem', { name: 'Phone' })).toBeDisabled()
    await user.click(screen.getByRole('menuitem', { name: 'Laptop' }))
    expect(mocks.peer).toHaveBeenCalledWith('entry', 'online')
  })
  it('sends to all devices only after choosing that option', async () => {
    const user = userEvent.setup()
    render(<ClipboardSendMenu entryId="entry" />)
    await user.click(screen.getByRole('button', { name: 'clipboard.contextMenu.send' }))
    expect(mocks.all).not.toHaveBeenCalled()
    await user.click(screen.getByRole('menuitem', { name: 'clipboard.contextMenu.sendAll' }))
    expect(mocks.all).toHaveBeenCalledWith('entry')
  })
})

it('expands the send label without adding a browser tooltip', async () => {
  render(<ClipboardSendMenu entryId="entry" />)
  const button = screen.getByRole('button', { name: 'clipboard.contextMenu.send' })
  expect(button).not.toHaveAttribute('title')
  const label = screen.getByText('clipboard.contextMenu.send')
  expect(label).toHaveAttribute('aria-hidden', 'true')
  fireEvent.pointerEnter(button, { pointerType: 'mouse' })
  await waitFor(() => expect(label).toHaveAttribute('aria-hidden', 'false'))
})

it('keeps the send label expanded while the menu is open, then collapses after dismissal', async () => {
  const user = userEvent.setup()
  render(<ClipboardSendMenu entryId="entry" />)
  const button = screen.getByRole('button', { name: 'clipboard.contextMenu.send' })
  const label = screen.getByText('clipboard.contextMenu.send')
  await user.click(button)
  await user.hover(screen.getByRole('menuitem', { name: 'Laptop' }))
  await act(async () => {
    await new Promise(resolve => setTimeout(resolve, 180))
  })
  expect(label).toHaveAttribute('aria-hidden', 'false')
  await user.keyboard('{Escape}')
  expect(button).toHaveFocus()
  await user.click(document.body)
  await waitFor(() => expect(label).toHaveAttribute('aria-hidden', 'true'))
})

it('opens by keyboard and closes when the trigger is clicked again', async () => {
  const user = userEvent.setup()
  render(<ClipboardSendMenu entryId="entry" />)
  const trigger = screen.getByRole('button', { name: 'clipboard.contextMenu.send' })
  await user.tab()
  await user.keyboard('{Enter}')
  expect(screen.getByRole('menu')).toBeInTheDocument()
  await user.click(trigger)
  expect(trigger).toHaveAttribute('aria-expanded', 'false')
})
