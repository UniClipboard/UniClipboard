import { Link, makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import type { InsetListProps } from '@/ui-kit/contract'
import { dotClass, textClass, useStatusStyles } from './status-styles'

// A WinUI settings-card group: one card layer, rows split by dividers, a status dot per row and a
// HyperlinkButton for the row's action.
const useStyles = makeStyles({
  list: {
    margin: 0,
    padding: 0,
    listStyle: 'none',
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusMedium,
  },
  row: {
    boxSizing: 'border-box',
    minHeight: '2.5rem',
    paddingInline: '1rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    ':not(:first-child)': { borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)` },
  },
  title: {
    flexGrow: 1,
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  detail: {
    flexShrink: 0,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  action: { flexShrink: 0, fontSize: tokens.fontSizeBase200 },
})

export function InsetList({ label, rows }: InsetListProps) {
  const styles = useStyles()
  const status = useStatusStyles()
  return (
    <ul className={styles.list} aria-label={label}>
      {rows.map(row => {
        const tone = row.tone ?? 'neutral'
        return (
          <li key={row.id} className={styles.row} data-tone={tone}>
            {!row.status && <span className={mergeClasses(status.dot, status[dotClass[tone]])} />}
            <span className={styles.title}>{row.title}</span>
            {row.status ??
              (row.detail ? (
                <span className={mergeClasses(styles.detail, status[textClass[tone]])}>
                  {row.detail}
                </span>
              ) : null)}
            {row.action ? (
              <Link as="button" className={styles.action} onClick={() => row.action?.onClick()}>
                {row.action.label}
              </Link>
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}
