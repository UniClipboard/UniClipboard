import { makeStyles, tokens } from '@fluentui/react-components'
import type { ActionBarProps } from '@/ui-kit/contract'

// Board WA2: the main actions of the detail pane are a WinUI CommandBar across the top of the
// pane, and the keyboard hint is a status strip along its bottom. Both are ordered against the
// pane's other children (a flex column), so the bar needs no position of its own.
const useStyles = makeStyles({
  bar: {
    order: -1,
    boxSizing: 'border-box',
    height: '3rem',
    flexShrink: 0,
    paddingInline: '0.75rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.25rem',
    backgroundColor: 'var(--w11-card-solid)',
    borderBottom: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    // A CommandBar's buttons are standard 32 px controls whatever size the caller asked for, which
    // sizes them for a floating bar on other platforms.
    '& .fui-Button': {
      minHeight: '2rem',
      height: '2rem',
      fontSize: tokens.fontSizeBase300,
      paddingInline: '0.75rem',
    },
  },
  status: {
    order: 1,
    boxSizing: 'border-box',
    height: '2rem',
    flexShrink: 0,
    paddingInline: '1rem',
    display: 'flex',
    alignItems: 'center',
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
})

export function ActionBar({ children, hint }: ActionBarProps) {
  const styles = useStyles()
  return (
    <>
      <div className={styles.bar}>{children}</div>
      {hint ? <div className={styles.status}>{hint}</div> : null}
    </>
  )
}
