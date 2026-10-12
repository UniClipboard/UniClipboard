import { makeStyles, tokens } from '@fluentui/react-components'
import { useId } from 'react'
import type { SettingsContentProps, SettingsGroupProps, SettingsRowProps } from '@/ui-kit/contract'
import './settings.css'

// WinUI SettingsCard stack (Win11 boards SetWin1-3): every setting is its own card, 4 px apart, under
// a semibold group heading; the page is one left-aligned column beside the sidebar. The design's
// extras that the sections have no data or control for (find-a-setting, identity header, per-row
// icons, On/Off text) are not drawn.
const useStyles = makeStyles({
  content: {
    flexGrow: 1,
    minHeight: 0,
    overflowY: 'auto',
    boxSizing: 'border-box',
    paddingInline: '1.25rem',
    paddingBlockEnd: '1.5rem',
  },
  column: {
    maxWidth: '45rem',
    display: 'flex',
    flexDirection: 'column',
    gap: '1.25rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
  },
  group: { minWidth: 0, margin: 0, padding: 0, border: 0 },
  title: {
    padding: '0 0.125rem 0.375rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    fontWeight: tokens.fontWeightSemibold,
  },
  stack: { minWidth: 0, display: 'flex', flexDirection: 'column', gap: '0.25rem' },
  row: {
    minHeight: '4.25rem',
    boxSizing: 'border-box',
    padding: '0.75rem 1rem',
    display: 'flex',
    flexWrap: 'wrap',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: '0.5rem 1rem',
    border: `${tokens.strokeWidthThin} solid var(--w11-card-stroke)`,
    borderRadius: tokens.borderRadiusMedium,
    backgroundColor: 'var(--w11-card)',
    backgroundClip: 'padding-box',
  },
  rowBare: { minHeight: '3.5rem' },
  text: {
    flex: '1 1 14rem',
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: '0.125rem',
  },
  label: { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '0.375rem' },
  description: {
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
    overflowWrap: 'anywhere',
  },
  control: {
    minWidth: 0,
    maxWidth: '100%',
    marginInlineStart: 'auto',
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
  },
  controlBare: { flex: '1 1 auto', marginInlineStart: 0 },
})

/**
 * Scrolling body of a settings category: the column of groups under the pane heading. The column
 * carries `data-settings-column`, which `settings.css` uses to give the sections' own (shadcn)
 * controls the Fluent tokens; popups the sections open portal into the page's `.settings-popups`.
 */
export function SettingsContent({ label, viewportRef, children }: SettingsContentProps) {
  const styles = useStyles()
  return (
    <div ref={viewportRef} role="region" aria-label={label} className={styles.content}>
      <div className={styles.column} data-settings-column="">
        {children}
      </div>
    </div>
  )
}

export function SettingsGroup({ title, children }: SettingsGroupProps) {
  const styles = useStyles()
  return (
    <fieldset className={styles.group}>
      {title && <legend className={styles.title}>{title}</legend>}
      <div className={styles.stack} data-settings-stack="">
        {children}
      </div>
    </fieldset>
  )
}

export function SettingsRow({ label, labelExtra, description, children }: SettingsRowProps) {
  const styles = useStyles()
  const labelId = useId()
  const hasText = Boolean(label || description)
  return (
    <div
      className={hasText ? styles.row : `${styles.row} ${styles.rowBare}`}
      data-settings-row=""
      role={label ? 'group' : undefined}
      aria-labelledby={label ? labelId : undefined}
    >
      {hasText && (
        <div className={styles.text}>
          {label && (
            <span id={labelId} className={styles.label}>
              {label}
              {labelExtra}
            </span>
          )}
          {description && <span className={styles.description}>{description}</span>}
        </div>
      )}
      {children && (
        <div className={hasText ? styles.control : `${styles.control} ${styles.controlBare}`}>
          {children}
        </div>
      )}
    </div>
  )
}
