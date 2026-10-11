import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { PaneProps } from '@/ui-kit/contract'

// Columns of the content card. The list column is a fixed-ish master column that may shrink; the
// detail column takes the rest. A divider separates them, as between WinUI ListDetails columns.
const useStyles = makeStyles({
  pane: {
    position: 'relative',
    boxSizing: 'border-box',
    minWidth: 0,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
  },
  list: {
    flex: '0 1 26rem',
    minWidth: '18rem',
    borderInlineEndWidth: tokens.strokeWidthThin,
    borderInlineEndStyle: 'solid',
    borderInlineEndColor: 'var(--w11-divider)',
  },
  detail: { flex: '1 1 21rem', minWidth: '18rem' },
})

export function Pane({ kind, label, children }: PaneProps) {
  const styles = useStyles()
  return (
    <section
      className={mergeClasses(styles.pane, styles[kind])}
      data-pane={kind}
      aria-label={label}
    >
      {children}
    </section>
  )
}
