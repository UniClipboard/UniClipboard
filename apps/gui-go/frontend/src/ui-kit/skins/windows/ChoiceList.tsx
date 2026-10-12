import { makeStyles, tokens } from '@fluentui/react-components'
import { ChevronRightRegular } from '@fluentui/react-icons'
import { createElement, useId } from 'react'
import type { ChoiceListProps } from '@/ui-kit/contract'
import { iconFor } from './icons'

// WinUI SettingsCard used as a navigation card: icon, header and description, chevron; cards are
// separate with a small gap, as in Windows Settings.
const useStyles = makeStyles({
  list: { display: 'flex', flexDirection: 'column', gap: '0.25rem' },
  title: {
    margin: '0.75rem 0 0.25rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  card: {
    boxSizing: 'border-box',
    width: '100%',
    minHeight: '4.25rem',
    padding: '0.75rem 1rem',
    display: 'flex',
    alignItems: 'center',
    gap: '1rem',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-card)',
    backgroundClip: 'padding-box',
    color: tokens.colorNeutralForeground1,
    fontFamily: 'inherit',
    textAlign: 'start',
    cursor: 'default',
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    ':active': { backgroundColor: tokens.colorSubtleBackgroundPressed },
    ':focus-visible': {
      outline: `2px solid ${tokens.colorStrokeFocus2}`,
      outlineOffset: '1px',
    },
    ':disabled': { color: tokens.colorNeutralForegroundDisabled },
  },
  icon: { display: 'inline-flex', flexShrink: 0, fontSize: '1.25rem' },
  text: { flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column' },
  header: { fontSize: tokens.fontSizeBase300, lineHeight: tokens.lineHeightBase300 },
  description: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  chevron: { display: 'inline-flex', flexShrink: 0, color: tokens.colorNeutralForeground2 },
})

export function ChoiceList({ label, title, items, onSelect, disabled }: ChoiceListProps) {
  const styles = useStyles()
  const headingId = useId()
  return (
    <section
      className={styles.list}
      aria-label={title ? undefined : label}
      aria-labelledby={title ? headingId : undefined}
    >
      {title && (
        <h2 id={headingId} className={styles.title}>
          {title}
        </h2>
      )}
      {items.map(item => (
        <button
          key={item.id}
          type="button"
          className={styles.card}
          disabled={disabled}
          onClick={() => onSelect(item.id)}
        >
          {item.icon && (
            <span className={styles.icon} aria-hidden="true">
              {createElement(iconFor(item.icon))}
            </span>
          )}
          <span className={styles.text}>
            <span className={styles.header}>{item.title}</span>
            {item.description && <span className={styles.description}>{item.description}</span>}
          </span>
          <span className={styles.chevron} aria-hidden="true">
            <ChevronRightRegular />
          </span>
        </button>
      ))}
    </section>
  )
}
