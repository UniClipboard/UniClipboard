import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it } from 'vitest'
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'

afterEach(cleanup)

describe('modal window dragging', () => {
  it.each(['dialog', 'alert-dialog'] as const)(
    '%s exposes its backdrop to native dragging without making its controls draggable',
    async kind => {
      render(
        kind === 'dialog' ? (
          <Dialog defaultOpen>
            <DialogContent>
              <DialogTitle>Settings</DialogTitle>
              <input aria-label="Name" />
            </DialogContent>
          </Dialog>
        ) : (
          <AlertDialog defaultOpen>
            <AlertDialogContent>
              <AlertDialogTitle>Confirm</AlertDialogTitle>
              <input aria-label="Name" />
              <AlertDialogCancel>Cancel</AlertDialogCancel>
            </AlertDialogContent>
          </AlertDialog>
        )
      )

      const backdrop = document.querySelector(`[data-slot="${kind}-overlay"]`)
      expect(backdrop).toHaveAttribute('data-tauri-drag-region')
      expect(backdrop?.closest('[inert]')).toBeNull()
      const input = screen.getByRole('textbox', { name: 'Name' })
      expect(input.closest('[data-tauri-drag-region]')).toBeNull()
      await userEvent.type(input, 'Example')
      expect(input).toHaveValue('Example')
      fireEvent.mouseDown(backdrop!, { button: 0, detail: 1 })
      expect(screen.getByRole(kind === 'dialog' ? 'dialog' : 'alertdialog')).toBeVisible()
      await userEvent.click(
        screen.getByRole('button', { name: kind === 'dialog' ? 'Close' : 'Cancel' })
      )
      await waitFor(() =>
        expect(screen.queryByRole(kind === 'dialog' ? 'dialog' : 'alertdialog')).toBeNull()
      )
    }
  )
})
