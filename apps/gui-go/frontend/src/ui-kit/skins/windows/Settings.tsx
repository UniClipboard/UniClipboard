import { makeStyles, tokens } from '@fluentui/react-components'
import { useId } from 'react'
import type { SettingsContentProps, SettingsGroupProps, SettingsRowProps } from '@/ui-kit/contract'

// Minimal WinUI SettingsCard stack. The Windows build keeps its existing settings page, so these
// exist to satisfy the contract and are not mounted by it.
const useStyles = makeStyles({
  content: { height: '100%', overflowY: 'auto', padding: '1.5rem' },
  group: { margin: '0 0 1.5rem', padding: 0, border: 0, minWidth: 0 },
  title: { padding: 0, marginBottom: '0.5rem', fontWeight: tokens.fontWeightSemibold },
  card: {
    backgroundColor: 'var(--w11-card)',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusMedium,
  },
  row: {
    minHeight: '3rem',
    paddingInline: '1rem',
    display: 'flex',
    alignItems: 'center',
    gap: '1rem',
    ':not(:first-child)': { borderTop: `${tokens.strokeWidthThin} solid var(--w11-divider)` },
  },
  text: { flexGrow: 1, minWidth: 0, display: 'flex', flexDirection: 'column' },
  description: { fontSize: tokens.fontSizeBase200, color: tokens.colorNeutralForeground2 },
  control: { flexShrink: 0, display: 'flex', alignItems: 'center', gap: '0.5rem' },
})

export function SettingsContent({ label, viewportRef, children }: SettingsContentProps) {
  const styles = useStyles()
  return (
    <div ref={viewportRef} role="region" aria-label={label} className={styles.content}>
      {children}
    </div>
  )
}

export function SettingsGroup({ title, children }: SettingsGroupProps) {
  const styles = useStyles()
  return (
    <fieldset className={styles.group}>
      {title && <legend className={styles.title}>{title}</legend>}
      <div className={styles.card}>{children}</div>
    </fieldset>
  )
}

export function SettingsRow({ label, labelExtra, description, children }: SettingsRowProps) {
  const styles = useStyles()
  const id = useId()
  return (
    <div className={styles.row}>
      <div className={styles.text}>
        {label && (
          <span id={id}>
            {label}
            {labelExtra}
          </span>
        )}
        {description && <span className={styles.description}>{description}</span>}
      </div>
      {children && <div className={styles.control}>{children}</div>}
    </div>
  )
}
