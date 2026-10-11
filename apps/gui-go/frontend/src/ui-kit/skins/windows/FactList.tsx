import { makeStyles, tokens } from '@fluentui/react-components'
import type { FactListProps } from '@/ui-kit/contract'

// Small facts side by side as WinUI SettingsCard tiles (board C4): label above, value below.
const useStyles = makeStyles({
  list: { display: 'flex', flexWrap: 'wrap', gap: '0.5rem', margin: 0 },
  fact: {
    minWidth: '7rem',
    padding: '0.625rem 0.875rem',
    borderRadius: tokens.borderRadiusXLarge,
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
  },
  label: { fontSize: tokens.fontSizeBase200, color: tokens.colorNeutralForeground2 },
  value: { margin: 0, fontSize: tokens.fontSizeBase300, fontWeight: tokens.fontWeightSemibold },
  mono: { fontFamily: tokens.fontFamilyMonospace },
})

export function FactList({ label, facts }: FactListProps) {
  const styles = useStyles()
  return (
    <dl aria-label={label} className={styles.list}>
      {facts.map(fact => (
        <div key={fact.id} className={styles.fact}>
          <dt className={styles.label}>{fact.label}</dt>
          <dd className={`${styles.value} ${fact.monospace ? styles.mono : ''}`}>{fact.value}</dd>
        </div>
      ))}
    </dl>
  )
}
