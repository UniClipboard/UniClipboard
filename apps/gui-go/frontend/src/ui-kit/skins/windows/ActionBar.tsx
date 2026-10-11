import { makeStyles, tokens } from '@fluentui/react-components'
import type { ActionBarProps } from '@/ui-kit/contract'

// WinUI CommandBarFlyout-style floating bar: acrylic layer, overlay corner radius and the flyout
// elevation, docked at the bottom of its pane.
const useStyles = makeStyles({
  bar: {
    position: 'absolute',
    insetInline: '1rem',
    bottom: '1rem',
    zIndex: 2,
    boxSizing: 'border-box',
    minHeight: '3rem',
    paddingBlock: '0.25rem',
    paddingInline: '0.5rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.25rem',
    backgroundColor: 'var(--w11-acrylic)',
    backdropFilter: 'blur(30px) saturate(125%)',
    border: `${tokens.strokeWidthThin} solid var(--w11-stroke-elevation)`,
    borderRadius: tokens.borderRadiusXLarge,
    boxShadow: tokens.shadow16,
  },
  hint: {
    marginInlineStart: 'auto',
    paddingInline: '0.5rem',
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
    <div className={styles.bar}>
      {children}
      {hint ? <span className={styles.hint}>{hint}</span> : null}
    </div>
  )
}
