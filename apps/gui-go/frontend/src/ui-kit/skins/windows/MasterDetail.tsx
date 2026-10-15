import { makeStyles, tokens } from '@fluentui/react-components'
import type { MasterDetailProps } from '@/ui-kit/contract'

// A list pane beside a detail pane inside a ContentDialog, as in a WinUI ListDetailsView. The dialog
// sizes to its content, so the view has a fixed height and each pane scrolls by itself.
const useStyles = makeStyles({
  root: {
    display: 'flex',
    height: '30rem',
    minHeight: 0,
    overflow: 'hidden',
    borderRadius: tokens.borderRadiusMedium,
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  list: {
    width: '15rem',
    flexShrink: 0,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    backgroundColor: 'var(--w11-base)',
    borderRight: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
  },
  detail: { flexGrow: 1, minWidth: 0, overflowY: 'auto' },
  body: {
    boxSizing: 'border-box',
    maxWidth: '48rem',
    padding: '1.25rem 1.75rem 1.75rem',
    display: 'flex',
    flexDirection: 'column',
    gap: '1.125rem',
  },
})

export function MasterDetail({ list, detail, detailLabel }: MasterDetailProps) {
  const styles = useStyles()
  return (
    <div className={styles.root}>
      <div className={styles.list}>{list}</div>
      <section className={styles.detail} aria-label={detailLabel}>
        <div className={styles.body}>{detail}</div>
      </section>
    </div>
  )
}
