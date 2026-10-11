import { makeStyles, tokens } from '@fluentui/react-components'
import { createElement } from 'react'
import type { ContentUnavailableProps } from '@/ui-kit/contract'
import { iconFor } from './icons'

// Empty state as in Windows 11 apps: large regular glyph, Subtitle title, Body description.
const useStyles = makeStyles({
  root: {
    boxSizing: 'border-box',
    flexGrow: 1,
    padding: '1.5rem',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: '0.5rem',
    textAlign: 'center',
  },
  icon: {
    display: 'inline-flex',
    marginBottom: '0.5rem',
    fontSize: '3rem',
    color: tokens.colorNeutralForeground3,
  },
  title: {
    fontSize: tokens.fontSizeBase500,
    lineHeight: tokens.lineHeightBase500,
    fontWeight: tokens.fontWeightSemibold,
    color: tokens.colorNeutralForeground1,
  },
  description: {
    maxWidth: '24rem',
    fontSize: tokens.fontSizeBase300,
    lineHeight: tokens.lineHeightBase300,
    color: tokens.colorNeutralForeground2,
  },
  actions: {
    marginTop: '1rem',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'stretch',
    gap: '0.5rem',
    minWidth: 'min(17.5rem, 100%)',
  },
})

export function ContentUnavailable({
  icon,
  title,
  description,
  children,
}: ContentUnavailableProps) {
  const styles = useStyles()
  return (
    <div className={styles.root} role="status">
      {icon ? (
        <span className={styles.icon} aria-hidden="true">
          {createElement(iconFor(icon))}
        </span>
      ) : null}
      <span className={styles.title}>{title}</span>
      {description ? <span className={styles.description}>{description}</span> : null}
      {children ? <div className={styles.actions}>{children}</div> : null}
    </div>
  )
}
