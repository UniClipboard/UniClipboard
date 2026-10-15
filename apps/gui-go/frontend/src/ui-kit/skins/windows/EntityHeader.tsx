import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { createElement } from 'react'
import type { EntityHeaderProps } from '@/ui-kit/contract'
import { iconFor } from './icons'
import { dotClass, textClass, useStatusStyles } from './status-styles'

// The heading of a detail page in WinUI Settings (a device page): a large icon on a card tile, the
// Subtitle-sized name, a Caption status line and the commands at the end.
const useStyles = makeStyles({
  root: { display: 'flex', alignItems: 'center', gap: '1rem' },
  tile: {
    flexShrink: 0,
    width: '3.5rem',
    height: '3.5rem',
    borderRadius: tokens.borderRadiusLarge,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    fontSize: '2rem',
  },
  pending: {
    backgroundColor: 'transparent',
    border: `${tokens.strokeWidthThin} dashed ${tokens.colorNeutralStroke1}`,
    color: tokens.colorBrandForeground1,
  },
  text: { flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column', gap: '0.25rem' },
  titleRow: { display: 'flex', alignItems: 'center', gap: '0.5rem', minWidth: 0 },
  title: {
    margin: 0,
    fontSize: tokens.fontSizeBase500,
    lineHeight: tokens.lineHeightBase500,
    fontWeight: tokens.fontWeightSemibold,
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
  },
  badge: {
    flexShrink: 0,
    paddingInline: '0.5rem',
    borderRadius: tokens.borderRadiusCircular,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  status: {
    display: 'flex',
    alignItems: 'center',
    gap: '0.375rem',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
    fontVariantNumeric: 'tabular-nums',
  },
  statusText: { fontWeight: tokens.fontWeightSemibold },
  description: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  actions: { flexShrink: 0, display: 'flex', gap: '0.5rem' },
})

export function EntityHeader({
  icon,
  pending,
  title,
  badge,
  status,
  description,
  actions,
}: EntityHeaderProps) {
  const styles = useStyles()
  const statusStyles = useStatusStyles()
  return (
    <div className={styles.root}>
      <span className={mergeClasses(styles.tile, pending && styles.pending)}>
        {createElement(iconFor(icon))}
      </span>
      <div className={styles.text}>
        <span className={styles.titleRow}>
          <h2 className={styles.title} title={title}>
            {title}
          </h2>
          {badge && <span className={styles.badge}>{badge}</span>}
        </span>
        {status && (
          <span className={styles.status}>
            <span className={mergeClasses(statusStyles.dot, statusStyles[dotClass[status.tone]])} />
            <span
              className={mergeClasses(
                styles.statusText,
                (status.tone === 'warning' || status.tone === 'critical') &&
                  statusStyles[textClass[status.tone]]
              )}
            >
              {status.text}
            </span>
            {status.detail && (
              <>
                <span aria-hidden="true">·</span>
                <span>{status.detail}</span>
              </>
            )}
          </span>
        )}
        {description && <span className={styles.description}>{description}</span>}
      </div>
      {actions && <div className={styles.actions}>{actions}</div>}
    </div>
  )
}
