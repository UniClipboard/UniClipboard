import {
  Button as FluentButton,
  Dialog,
  DialogSurface,
  DialogTitle,
  makeStyles,
  tokens,
} from '@fluentui/react-components'
import { ChevronLeftRegular, ChevronRightRegular, DismissRegular } from '@fluentui/react-icons'
import type { KeyboardEvent } from 'react'
import type { QuickLookProps } from '@/ui-kit/contract'

// A large preview over the window in the Win11 overlay style (boards T3/C5): smoke backdrop, a dialog surface
// with a title bar (position, commands, close), the picture centred on the dark stage, and previous/next
// buttons at its sides. Escape or Space closes, the arrow keys step, Enter runs the primary command.
const useStyles = makeStyles({
  surface: {
    padding: 0,
    width: 'min(64rem, calc(100vw - 4rem))',
    maxWidth: 'none',
    height: 'min(44rem, calc(100vh - 4rem))',
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
  },
  bar: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
    minHeight: '3rem',
    paddingInline: '1rem 0.5rem',
    borderBottom: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
  },
  title: {
    flexGrow: 1,
    margin: 0,
    fontSize: tokens.fontSizeBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  stage: {
    position: 'relative',
    flexGrow: 1,
    minHeight: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: 'var(--w11-code-bg)',
  },
  picture: { maxWidth: 'calc(100% - 7rem)', maxHeight: 'calc(100% - 2rem)', objectFit: 'contain' },
  side: { position: 'absolute', top: 'calc(50% - 1.25rem)' },
  previous: { insetInlineStart: '0.75rem' },
  next: { insetInlineEnd: '0.75rem' },
})

export function QuickLook({
  open,
  onOpenChange,
  title,
  closeLabel,
  src,
  alt,
  previous,
  next,
  primary,
  actions,
}: QuickLookProps) {
  const styles = useStyles()
  const onKeyDown = (event: KeyboardEvent) => {
    if ((event.target as HTMLElement).closest('input, textarea')) return
    if (event.key === ' ') {
      event.preventDefault()
      onOpenChange(false)
    } else if (event.key === 'ArrowLeft' && previous) {
      event.preventDefault()
      previous.onClick()
    } else if (event.key === 'ArrowRight' && next) {
      event.preventDefault()
      next.onClick()
    } else if (event.key === 'Enter' && primary && event.target === event.currentTarget) {
      event.preventDefault()
      primary.onClick()
    }
  }
  return (
    <Dialog open={open} onOpenChange={(_, data) => onOpenChange(data.open)}>
      <DialogSurface className={styles.surface} onKeyDown={onKeyDown} aria-label={title}>
        <div className={styles.bar}>
          <DialogTitle className={styles.title}>{title}</DialogTitle>
          {actions}
          {primary ? (
            <FluentButton appearance="primary" onClick={() => primary.onClick()}>
              {primary.label}
            </FluentButton>
          ) : null}
          <FluentButton
            appearance="subtle"
            aria-label={closeLabel}
            icon={<DismissRegular />}
            onClick={() => onOpenChange(false)}
          />
        </div>
        <div className={styles.stage}>
          {src ? <img className={styles.picture} src={src} alt={alt} /> : null}
          {previous ? (
            <FluentButton
              className={`${styles.side} ${styles.previous}`}
              shape="circular"
              aria-label={previous.label}
              icon={<ChevronLeftRegular />}
              onClick={() => previous.onClick()}
            />
          ) : null}
          {next ? (
            <FluentButton
              className={`${styles.side} ${styles.next}`}
              shape="circular"
              aria-label={next.label}
              icon={<ChevronRightRegular />}
              onClick={() => next.onClick()}
            />
          ) : null}
        </div>
      </DialogSurface>
    </Dialog>
  )
}
