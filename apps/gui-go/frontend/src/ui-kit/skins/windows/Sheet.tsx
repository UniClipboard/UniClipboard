import {
  Button as FluentButton,
  Dialog,
  DialogBody,
  DialogSurface,
  DialogTitle,
  makeStyles,
  mergeClasses,
  tokens,
} from '@fluentui/react-components'
import { useEffect, useLayoutEffect, useRef } from 'react'
import type { SheetProps } from '@/ui-kit/contract'
import { lastConnectedFocus, trackFocusHistory } from './focus-history'

// Board WC3: a large WinUI ContentDialog. Title and header controls on the solid layer, the content
// below, and a footer band on the base layer with the status text at the start and the closing
// button at the end.
const useStyles = makeStyles({
  task: { width: 'min(38rem, calc(100vw - 4rem))' },
  surface: {
    padding: 0,
    width: 'min(50rem, calc(100vw - 4rem))',
    maxWidth: 'none',
    maxHeight: 'calc(100vh - 4rem)',
    overflow: 'hidden',
    display: 'flex',
  },
  body: {
    display: 'flex',
    flexDirection: 'column',
    gap: 0,
    flexGrow: 1,
    minHeight: 0,
    maxHeight: 'none',
  },
  title: { padding: '1.5rem 1.5rem 0.75rem', fontSize: tokens.fontSizeBase500 },
  header: { paddingInline: '1.5rem', paddingBottom: '0.75rem', display: 'flex', gap: '0.5rem' },
  content: {
    flexGrow: 1,
    minHeight: 0,
    overflowY: 'auto',
    paddingInline: '1.5rem',
    paddingBottom: '1.25rem',
  },
  footer: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    padding: '1.25rem 1.5rem',
    backgroundColor: 'var(--w11-base)',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  footerStart: {
    flexGrow: 1,
    minWidth: 0,
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
    fontSize: tokens.fontSizeBase200,
    color: tokens.colorNeutralForeground2,
  },
  done: { minWidth: '7.5rem' },
})

/**
 * WinUI ContentDialog for a focused task. Fluent's Dialog traps focus; focus returns to the control
 * that opened it (recorded before the surface takes focus), or to the last focused element still in
 * the page when the opener is gone.
 */
export function Sheet({
  variant = 'management',
  open,
  onOpenChange,
  title,
  header,
  children,
  footer,
  done,
  onClosed,
}: SheetProps) {
  const styles = useStyles()
  const opener = useRef<HTMLElement | null>(null)
  const surface = useRef<HTMLDivElement | null>(null)
  useEffect(() => trackFocusHistory(), [])
  useLayoutEffect(() => {
    if (open) opener.current = lastConnectedFocus()
  }, [open])
  const finishMotion = (_: null, data: { direction: 'enter' | 'exit' }) => {
    if (data.direction !== 'exit') return
    const target = opener.current?.isConnected
      ? opener.current
      : lastConnectedFocus(surface.current)
    target?.focus()
    opener.current = null
    onClosed?.()
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(_, data) => onOpenChange(data.open)}
      surfaceMotion={{ onMotionFinish: finishMotion }}
    >
      <DialogSurface
        ref={surface}
        className={mergeClasses(styles.surface, variant === 'task' && styles.task)}
      >
        <DialogBody className={styles.body}>
          <DialogTitle className={styles.title}>{title}</DialogTitle>
          {header ? <div className={styles.header}>{header}</div> : null}
          <div className={styles.content}>{children}</div>
          {(footer || done) && (
            <div className={styles.footer}>
              <div className={styles.footerStart}>{footer}</div>
              {done && (
                <FluentButton className={styles.done} onClick={() => done.onClick()}>
                  {done.label}
                </FluentButton>
              )}
            </div>
          )}
        </DialogBody>
      </DialogSurface>
    </Dialog>
  )
}
