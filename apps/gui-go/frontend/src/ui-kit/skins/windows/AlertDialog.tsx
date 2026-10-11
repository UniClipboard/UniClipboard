import {
  Button as FluentButton,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  makeStyles,
  tokens,
} from '@fluentui/react-components'
import { useLayoutEffect, useRef } from 'react'
import type { AlertDialogProps } from '@/ui-kit/contract'

// ContentDialog layout: content on the solid layer, buttons in a footer band of equal columns.
const useStyles = makeStyles({
  surface: { padding: 0, maxWidth: '28rem', overflow: 'hidden' },
  body: { display: 'flex', flexDirection: 'column', gap: 0 },
  title: { padding: '1.5rem 1.5rem 0.75rem' },
  content: { padding: '0 1.5rem 1.5rem' },
  actions: {
    display: 'grid',
    gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
    gap: '0.5rem',
    padding: '1.5rem',
    backgroundColor: 'var(--w11-base)',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
})

/**
 * WinUI ContentDialog: smoke backdrop, solid surface, equal-width buttons in the footer with the
 * confirming action as the accent button. Windows does not colour destructive confirmations red,
 * so `destructive` keeps the accent style; the text carries the warning.
 */
export function AlertDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  cancelLabel,
  onConfirm,
}: AlertDialogProps) {
  const styles = useStyles()
  // Fluent restores focus only to a DialogTrigger; this dialog is opened by the caller's own
  // control. The layout effect records the opener before the surface moves focus into itself, and
  // focus goes back once the exit motion has finished and the page is no longer inert.
  const opener = useRef<HTMLElement | null>(null)
  useLayoutEffect(() => {
    if (open && document.activeElement instanceof HTMLElement)
      opener.current = document.activeElement
  }, [open])
  const restoreFocus = (_: null, data: { direction: 'enter' | 'exit' }) => {
    if (data.direction !== 'exit') return
    if (opener.current?.isConnected) opener.current.focus()
    opener.current = null
  }
  return (
    <Dialog
      modalType="alert"
      open={open}
      onOpenChange={(_, data) => onOpenChange(data.open)}
      surfaceMotion={{ onMotionFinish: restoreFocus }}
    >
      <DialogSurface className={styles.surface}>
        <DialogBody className={styles.body}>
          <DialogTitle className={styles.title}>{title}</DialogTitle>
          <DialogContent className={styles.content}>{description}</DialogContent>
          <DialogActions fluid className={styles.actions}>
            <FluentButton
              appearance="primary"
              onClick={() => {
                onConfirm()
                onOpenChange(false)
              }}
            >
              {confirmLabel}
            </FluentButton>
            <FluentButton appearance="secondary" onClick={() => onOpenChange(false)}>
              {cancelLabel}
            </FluentButton>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  )
}
