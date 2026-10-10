import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { domMax, LazyMotion } from 'framer-motion'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
  ContextMenuSub,
  ContextMenuSubTrigger,
  ContextMenuSubContent,
  ContextMenuCheckboxItem,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
} from '@/components/motion/context-menu'

function setup() {
  const select = vi.fn()
  render(
    <LazyMotion features={domMax} strict>
      <ContextMenu>
        <ContextMenuTrigger>
          <button type="button">Entry</button>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuItem disabled onSelect={select}>
            Unavailable
          </ContextMenuItem>
          <ContextMenuItem textValue="Copy" onSelect={select}>
            Copy
          </ContextMenuItem>
          <ContextMenuSub>
            <ContextMenuSubTrigger textValue="Send">Send</ContextMenuSubTrigger>
            <ContextMenuSubContent>
              <ContextMenuItem onSelect={select}>Desk Mac</ContextMenuItem>
            </ContextMenuSubContent>
          </ContextMenuSub>
          <ContextMenuItem tone="destructive">Delete</ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </LazyMotion>
  )
  return { select, trigger: screen.getByRole('button', { name: 'Entry' }) }
}

function Choices() {
  const [checked, setChecked] = useState(false)
  const [value, setValue] = useState('one')
  return (
    <ContextMenu>
      <ContextMenuTrigger>
        <button type="button">Choices</button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuCheckboxItem
          checked={checked}
          onCheckedChange={setChecked}
          closeOnSelect={false}
        >
          Offline
        </ContextMenuCheckboxItem>
        <ContextMenuRadioGroup value={value} onValueChange={setValue}>
          <ContextMenuRadioItem value="one" closeOnSelect={false}>
            One
          </ContextMenuRadioItem>
          <ContextMenuRadioItem value="two" closeOnSelect={false}>
            Two
          </ContextMenuRadioItem>
        </ContextMenuRadioGroup>
      </ContextMenuContent>
    </ContextMenu>
  )
}

describe('beUI context menu', () => {
  it('uses native menu buttons, skips disabled rows and selects with the keyboard', async () => {
    const { trigger, select } = setup()
    trigger.focus()
    fireEvent.keyDown(trigger, { key: 'F10', shiftKey: true })
    const copy = await screen.findByRole('menuitem', { name: 'Copy' })
    expect(copy.tagName).toBe('BUTTON')
    await waitFor(() => expect(copy).toHaveFocus())
    await userEvent.keyboard('{End}{ArrowDown}')
    expect(copy).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    expect(select).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('keeps the submenu inside the same menu tree and closes all menus after selecting', async () => {
    const { trigger, select } = setup()
    fireEvent.contextMenu(trigger, { clientX: 30, clientY: 40 })
    await screen.findByRole('menu')
    await waitFor(() => expect(screen.getByRole('menuitem', { name: 'Copy' })).toHaveFocus())
    await userEvent.keyboard('s{ArrowRight}')
    const peer = await screen.findByRole('menuitem', { name: 'Desk Mac' })
    fireEvent.pointerDown(peer)
    expect(screen.getAllByRole('menu')).toHaveLength(2)
    fireEvent.click(peer)
    expect(select).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('closes only the submenu on Escape and prevents the panel from receiving the key', async () => {
    const { trigger } = setup()
    fireEvent.contextMenu(trigger)
    await screen.findByRole('menu')
    await waitFor(() => expect(screen.getByRole('menuitem', { name: 'Copy' })).toHaveFocus())
    await userEvent.keyboard('s{ArrowRight}')
    const peer = await screen.findByRole('menuitem', { name: 'Desk Mac' })
    await waitFor(() => expect(peer).toHaveFocus())
    const windowKey = vi.fn()
    window.addEventListener('keydown', windowKey)
    try {
      await userEvent.keyboard('{Escape}')
      expect(screen.getAllByRole('menu')).toHaveLength(1)
      expect(screen.getByRole('menuitem', { name: 'Send' })).toHaveFocus()
      expect(windowKey).not.toHaveBeenCalled()
      await userEvent.keyboard('{Escape}')
      expect(screen.queryByRole('menu')).not.toBeInTheDocument()
      expect(trigger).toHaveFocus()
    } finally {
      window.removeEventListener('keydown', windowKey)
    }
  })

  it('waits for hover intent again after leaving a submenu instead of replaying its entrance', async () => {
    const { trigger } = setup()
    fireEvent.contextMenu(trigger)
    const send = await screen.findByRole('menuitem', { name: 'Send' })
    const user = userEvent.setup()
    await user.hover(send)
    await screen.findByRole('menuitem', { name: 'Desk Mac' })
    await user.hover(screen.getByRole('menuitem', { name: 'Copy' }))
    expect(screen.queryByRole('menuitem', { name: 'Desk Mac' })).not.toBeInTheDocument()
    await user.hover(send)
    expect(screen.queryByRole('menuitem', { name: 'Desk Mac' })).not.toBeInTheDocument()
    await screen.findByRole('menuitem', { name: 'Desk Mac' })
    expect(screen.getAllByRole('menu')).toHaveLength(2)
  })

  it('supports checkbox and radio choices without closing', async () => {
    render(
      <LazyMotion features={domMax} strict>
        <Choices />
      </LazyMotion>
    )
    fireEvent.contextMenu(screen.getByRole('button', { name: 'Choices' }))
    const checkbox = await screen.findByRole('menuitemcheckbox')
    fireEvent.click(checkbox)
    expect(checkbox).toHaveAttribute('aria-checked', 'true')
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Two' }))
    expect(screen.getByRole('menuitemradio', { name: 'Two' })).toHaveAttribute(
      'aria-checked',
      'true'
    )
    expect(screen.getByRole('menuitemradio', { name: 'One' })).toHaveAttribute(
      'aria-checked',
      'false'
    )
    expect(screen.getByRole('menu')).toBeInTheDocument()
  })

  it('keeps the menu inside a small window at the click point', async () => {
    const bounds = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      left: 0,
      top: 0,
      x: 0,
      y: 0,
      width: 240,
      height: 248,
      right: 240,
      bottom: 248,
      toJSON: () => ({}),
    })
    vi.stubGlobal('innerWidth', 360)
    vi.stubGlobal('innerHeight', 320)
    try {
      const { trigger } = setup()
      fireEvent.contextMenu(trigger, { clientX: 350, clientY: 310 })
      const menu = await screen.findByRole('menu')
      await waitFor(() => expect(menu.parentElement).toHaveStyle({ left: '112px', top: '64px' }))
    } finally {
      bounds.mockRestore()
      vi.unstubAllGlobals()
    }
  })
})
