import {
  Button as FluentButton,
  Spinner,
  makeStyles,
  mergeClasses,
  tokens,
} from '@fluentui/react-components'
import { ArrowLeftRegular } from '@fluentui/react-icons'
import { createElement } from 'react'
import type { EntryPageProps } from '@/ui-kit/contract'
import { useElevationStyles } from './button-appearance'
import { iconFor } from './icons'

// Windows 11 setup page (design page "Setup · Startup · Unlock"): content leading-aligned in a
// centred column on the Mica base, WinUI Title over Body text, Back and the step's actions in the
// footer.
const useStyles = makeStyles({
  root: {
    boxSizing: 'border-box',
    display: 'flex',
    flex: '1 1 auto',
    flexDirection: 'column',
    minHeight: 0,
    width: '100%',
    backgroundColor: 'var(--w11-base)',
    color: tokens.colorNeutralForeground1,
    fontFamily: tokens.fontFamilyBase,
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
  },
  scroll: {
    flex: '1 1 auto',
    minHeight: 0,
    overflowY: 'auto',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    paddingInline: '2.5rem',
  },
  column: {
    boxSizing: 'border-box',
    width: '100%',
    maxWidth: '32.5rem',
    marginBlock: 'auto',
    paddingBlock: '1.5rem 1.75rem',
    display: 'flex',
    flexDirection: 'column',
  },
  header: { display: 'flex', flexDirection: 'column', alignItems: 'flex-start' },
  icon: { width: '2.5rem', height: '2.5rem' },
  status: {
    margin: '1.25rem 0 0',
    display: 'flex',
    alignItems: 'center',
    gap: '0.375rem',
    fontSize: tokens.fontSizeBase200,
    lineHeight: tokens.lineHeightBase200,
    color: tokens.colorNeutralForeground2,
  },
  positive: { color: 'var(--w11-status-positive)' },
  critical: { color: 'var(--w11-status-critical)' },
  statusIcon: { display: 'inline-flex', fontSize: '0.875rem' },
  title: {
    margin: '0.375rem 0 0',
    fontSize: tokens.fontSizeHero700,
    lineHeight: tokens.lineHeightHero700,
    fontWeight: tokens.fontWeightSemibold,
    overflowWrap: 'anywhere',
  },
  titleOnly: { marginTop: '1.25rem' },
  description: {
    margin: '0.5rem 0 0',
    color: tokens.colorNeutralForeground2,
    overflowWrap: 'anywhere',
  },
  body: {
    marginTop: '1.75rem',
    display: 'flex',
    flexDirection: 'column',
    gap: '1.25rem',
    '& form': { display: 'flex', flexDirection: 'column', gap: '1.25rem' },
  },
  footer: {
    flexShrink: 0,
    padding: '1.25rem 1.5rem 1.5rem',
    display: 'flex',
    alignItems: 'center',
    gap: '0.5rem',
  },
  spacer: { flex: '1 1 auto' },
})

export function EntryPage({
  appIcon,
  status,
  title,
  description,
  children,
  back,
  actions,
}: EntryPageProps) {
  const styles = useStyles()
  const elevation = useElevationStyles()
  const tone = status?.tone ?? 'neutral'
  return (
    <div className={styles.root}>
      <main className={styles.scroll}>
        <div className={styles.column}>
          <header className={styles.header}>
            <img src={appIcon} alt="" className={styles.icon} draggable={false} />
            {status && (
              <div className={mergeClasses(styles.status, tone !== 'neutral' && styles[tone])}>
                {status.busy ? (
                  <Spinner size="extra-tiny" />
                ) : (
                  status.icon && (
                    <span className={styles.statusIcon} aria-hidden="true">
                      {createElement(iconFor(status.icon))}
                    </span>
                  )
                )}
                <span>{status.text}</span>
              </div>
            )}
            <h1
              className={mergeClasses(styles.title, !status && styles.titleOnly)}
              aria-live="polite"
            >
              {title}
            </h1>
            {description && <p className={styles.description}>{description}</p>}
          </header>
          {children && <div className={styles.body}>{children}</div>}
        </div>
      </main>
      {(back || actions) && (
        <footer className={styles.footer}>
          {back && (
            <FluentButton
              appearance="secondary"
              icon={<ArrowLeftRegular />}
              disabled={back.disabled}
              onClick={() => back.onClick()}
              className={mergeClasses(elevation.text, elevation.raised)}
            >
              {back.label}
            </FluentButton>
          )}
          <span className={styles.spacer} />
          {actions}
        </footer>
      )}
    </div>
  )
}
