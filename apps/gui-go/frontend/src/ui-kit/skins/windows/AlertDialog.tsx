import {
  Button as FluentButton,
  Checkbox as FluentCheckbox,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Spinner,
  makeStyles,
  tokens,
} from '@fluentui/react-components'
import { useEffect, useLayoutEffect, useRef } from 'react'
import type { AlertDialogProps } from '@/ui-kit/contract'
import { lastConnectedFocus, trackFocusHistory } from './focus-history'

// ContentDialog layout: content on the solid layer, buttons in a footer band of equal columns.
const useStyles = makeStyles({
  surface: { padding: 0, maxWidth: '28rem', overflow: 'hidden' },
  body: { display: 'flex', flexDirection: 'column', gap: 0 },
  title: { padding: '1.5rem 1.5rem 0.75rem' },
  content: { padding: '0 1.5rem 1.5rem' },
  // ContentDialog content checkbox ("Don't ask again"), under the message.
  suppression: { display: 'flex', marginTop: '0.75rem', marginInlineStart: '-0.5rem' },
  field: { marginTop: '1rem' },
  error: { margin: '0.75rem 0 0', color: 'var(--w11-status-critical)' },
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
 * so `destructive` keeps the accent style; the text carries the warning, and initial focus goes to
 * Cancel so Enter does not destroy data (WinUI sets the close button as the default for these).
 */
export function AlertDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  confirmLabel,
  cancelLabel,
  destructive,
  onConfirm,
  suppression,
  field,
  onCancel,
  confirmDisabled,
  busyLabel,
  error,
}: AlertDialogProps) {
  const busy = busyLabel !== undefined
  const styles = useStyles()
  // Fluent restores focus only to a DialogTrigger; this dialog is opened by the caller's own
  // control. The layout effect records the opener before the surface moves focus into itself, and
  // focus goes back once the exit motion has finished and the page is no longer inert. An opener
  // that is gone by then (a menu item) falls back to the focus history.
  const opener = useRef<HTMLElement | null>(null)
  const surface = useRef<HTMLDivElement | null>(null)
  const cancel = useRef<HTMLButtonElement | null>(null)
  const confirm = useRef<HTMLButtonElement | null>(null)
  const input = useRef<HTMLInputElement | null>(null)
  useEffect(() => trackFocusHistory(), [])
  useLayoutEffect(() => {
    if (open) opener.current = lastConnectedFocus()
  }, [open])
  // Fluent focuses the first button from the surface's own effect, which runs after any autoFocus
  // and, because the surface is portalled, after this component's effects. A zero-delay task runs
  // after both and before the user can press Enter.
  // The default button gets initial focus even when the suppression checkbox comes first.
  useEffect(() => {
    if (!open) return
    const timer = setTimeout(() =>
      (field ? input : destructive ? cancel : confirm).current?.focus()
    )
    return () => clearTimeout(timer)
    // The field's presence is fixed for one opening; its value changes must not move focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, destructive])
  const restoreFocus = (_: null, data: { direction: 'enter' | 'exit' }) => {
    if (data.direction !== 'exit') return
    const target = opener.current?.isConnected
      ? opener.current
      : lastConnectedFocus(surface.current)
    target?.focus()
    opener.current = null
  }
  return (
    <Dialog
      modalType="alert"
      open={open}
      onOpenChange={(_, data) => {
        // Running work cannot be dismissed (Escape).
        if (!data.open && busy) return
        onOpenChange(data.open)
      }}
      surfaceMotion={{ onMotionFinish: restoreFocus }}
    >
      <DialogSurface ref={surface} className={styles.surface}>
        <DialogBody className={styles.body}>
          <DialogTitle className={styles.title}>{title}</DialogTitle>
          <DialogContent className={styles.content}>
            {description}
            {children}
            {suppression ? (
              <FluentCheckbox
                className={styles.suppression}
                label={suppression.label}
                checked={suppression.checked}
                onChange={(_, data) => suppression.onCheckedChange(data.checked === true)}
              />
            ) : null}
            {field ? (
              <Field label={field.label} className={styles.field}>
                <Input
                  ref={input}
                  value={field.value}
                  placeholder={field.placeholder}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                  onChange={(_, data) => field.onValueChange(data.value)}
                />
              </Field>
            ) : null}
            {error ? (
              <p className={styles.error} role="alert">
                {error}
              </p>
            ) : null}
          </DialogContent>
          <DialogActions fluid className={styles.actions}>
            <FluentButton
              appearance="primary"
              ref={confirm}
              disabled={confirmDisabled || busy}
              aria-busy={busy || undefined}
              icon={busy ? <Spinner size="extra-tiny" /> : undefined}
              onClick={() => {
                const result = onConfirm()
                if (!(result instanceof Promise)) onOpenChange(false)
              }}
            >
              {busy ? busyLabel : confirmLabel}
            </FluentButton>
            <FluentButton
              appearance="secondary"
              ref={cancel}
              disabled={busy}
              onClick={() => {
                onCancel?.()
                onOpenChange(false)
              }}
            >
              {cancelLabel}
            </FluentButton>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  )
}
