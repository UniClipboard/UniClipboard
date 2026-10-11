import { makeStyles, tokens, Toolbar as FluentToolbar } from '@fluentui/react-components'
import type { ToolbarProps } from '@/ui-kit/contract'

const useStyles = makeStyles({
  root: {
    gap: '0.25rem',
    padding: '0.5rem',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid ${tokens.colorNeutralStroke1}`,
  },
})

/** WinUI CommandBar: Fluent Toolbar keeps one tab stop and moves with the arrow keys. */
export function Toolbar({ label, children }: ToolbarProps) {
  const styles = useStyles()
  return (
    <FluentToolbar aria-label={label} className={styles.root}>
      {children}
    </FluentToolbar>
  )
}
