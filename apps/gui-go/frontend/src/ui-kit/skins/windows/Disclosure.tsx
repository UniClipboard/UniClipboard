import { makeStyles, tokens } from '@fluentui/react-components'
import { ChevronDownRegular } from '@fluentui/react-icons'
import type { DisclosureProps } from '@/ui-kit/contract'

// WinUI Expander: a card header with the chevron at the trailing end, the content in the card below.
const useStyles = makeStyles({
  root: {
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-card)',
    backgroundClip: 'padding-box',
    '&[open] > summary > span:last-child': { transform: 'rotate(180deg)' },
  },
  summary: {
    listStyle: 'none',
    minHeight: '3rem',
    paddingInline: '1rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.75rem',
    borderRadius: tokens.borderRadiusMedium,
    cursor: 'default',
    '::-webkit-details-marker': { display: 'none' },
    ':hover': { backgroundColor: tokens.colorSubtleBackgroundHover },
    ':focus-visible': { outline: `2px solid ${tokens.colorStrokeFocus2}`, outlineOffset: '1px' },
  },
  label: { flex: '1 1 auto' },
  chevron: {
    display: 'inline-flex',
    transitionProperty: 'transform',
    transitionDuration: tokens.durationFast,
    transitionTimingFunction: tokens.curveDecelerateMax,
    '@media (prefers-reduced-motion: reduce)': { transitionDuration: '0s' },
  },
  content: {
    borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)`,
    padding: '0.75rem 1rem',
  },
})

export function Disclosure({ label, defaultOpen, children }: DisclosureProps) {
  const styles = useStyles()
  return (
    <details className={styles.root} open={defaultOpen}>
      <summary className={styles.summary}>
        <span className={styles.label}>{label}</span>
        <span className={styles.chevron} aria-hidden="true">
          <ChevronDownRegular />
        </span>
      </summary>
      <div className={styles.content}>{children}</div>
    </details>
  )
}
