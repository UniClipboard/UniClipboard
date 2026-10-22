import { makeStyles, tokens } from '@fluentui/react-components'
import type { TaskPanelProps } from '@/ui-kit/contract'
import { Button } from './Button'
import { Text } from './Text'

const useStyles = makeStyles({
  panel: {
    display: 'flex',
    flexDirection: 'column',
    gap: '1.5rem',
    paddingTop: '0.75rem',
    boxSizing: 'border-box',
    minHeight: 'min(21rem, calc(100dvh - 10rem))',
    maxHeight: 'calc(100dvh - 10rem)',
    overflow: 'hidden',
  },
  header: {
    display: 'flex',
    flexDirection: 'column',
    gap: '0.5rem',
    maxWidth: '34rem',
    flexShrink: 0,
  },
  stage: { color: tokens.colorBrandForeground1 },
  content: {
    minHeight: 0,
    overflowY: 'auto',
    padding: '0.25rem',
    flexGrow: 1,
    maxWidth: '34rem',
    width: '100%',
    display: 'flex',
    flexDirection: 'column',
    gap: '1rem',
  },
  actions: {
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '1rem',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    paddingTop: '1rem',
  },
  primary: {
    display: 'flex',
    flexWrap: 'wrap',
    justifyContent: 'flex-end',
    gap: '0.5rem',
    marginInlineStart: 'auto',
  },
})

export function TaskPanel({ title, description, stage, children, back, actions }: TaskPanelProps) {
  const styles = useStyles()
  return (
    <section className={styles.panel}>
      <header className={styles.header}>
        {stage && (
          <span className={styles.stage}>
            <Text role="caption">{stage}</Text>
          </span>
        )}
        <Text role="title" emphasized>
          {title}
        </Text>
        {description && <Text tone="secondary">{description}</Text>}
      </header>
      <div className={styles.content}>{children}</div>
      <footer className={styles.actions}>
        {back && (
          <Button disabled={back.disabled} onClick={back.onClick}>
            {back.label}
          </Button>
        )}
        <div className={styles.primary}>{actions}</div>
      </footer>
    </section>
  )
}
