import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { LazyMotion, domMax } from 'framer-motion'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Dialog, DialogContent, DialogTitle } from '../dialog'
import { toast } from '../toast'
import { Toaster } from '../toaster'

function mount(modal = false) {
  return render(
    <LazyMotion features={domMax} strict>
      <Toaster />
      {modal && (
        <Dialog open>
          <DialogContent>
            <DialogTitle>Settings</DialogTitle>
          </DialogContent>
        </Dialog>
      )}
    </LazyMotion>
  )
}

afterEach(() => {
  act(() => {
    toast.dismiss()
  })
  cleanup()
  vi.useRealTimers()
})

describe('global animated toasts', () => {
  it('renders the new stack outside the app and keeps it accessible during a modal', async () => {
    const { container } = mount(true)
    act(() => {
      toast.error('Save failed')
    })
    const stack = screen.getByRole('list', { name: 'Notifications' })
    expect(container.contains(stack)).toBe(false)
    await waitFor(() => expect(screen.getByText('Save failed')).toBeVisible())
    expect(stack.closest('[inert], [aria-hidden="true"]')).toBeNull()
    expect(stack).toHaveAttribute('data-animated-toast-stack')
  })
  it('updates loading in place and starts the completion lifetime at the update', async () => {
    mount()
    let id: string | number = ''
    act(() => {
      id = toast.loading('Saving', { duration: 0 })
    })
    await waitFor(() => expect(screen.getByText('Saving')).toBeVisible())
    act(() => {
      toast.success('Saved', { id, duration: 100 })
    })
    expect(document.querySelectorAll('[data-toast-id]')).toHaveLength(1)
    await waitFor(() => expect(screen.queryByText('Saved')).toBeNull())
  })
  it('keeps the newest messages bounded and executes actions above an open modal', () => {
    mount(true)
    const action = vi.fn()
    act(() => {
      for (let i = 0; i < 8; i++) toast.message(`Message ${i}`, { duration: 0 })
      toast.error('Retry save', { duration: 0, action: { label: 'Retry', onClick: action } })
    })
    expect(screen.queryByText('Message 0')).toBeNull()
    expect(document.querySelectorAll('[data-toast-id]')).toHaveLength(4)
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(action).toHaveBeenCalledOnce()
  })
})
