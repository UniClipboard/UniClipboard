import { makeStyles, mergeClasses, tokens } from '@fluentui/react-components'
import { createElement } from 'react'
import type { IconProps } from '@/ui-kit/contract'
import { iconFor } from './icons'

const useStyles = makeStyles({
  icon: {
    display: 'inline-flex',
    flexShrink: 0,
    fontSize: '0.75rem',
    lineHeight: 0,
    verticalAlign: 'middle',
  },
  secondary: { color: tokens.colorNeutralForeground2 },
  highlight: { color: tokens.colorPaletteMarigoldForeground1 },
})

/**
 * Inline Fluent System Icon. A highlighted mark (e.g. pinned) uses the filled variant, as WinUI
 * shows an active state, in the marigold attention colour.
 */
export function Icon({ name, label, tone }: IconProps) {
  const styles = useStyles()
  return (
    <span
      className={mergeClasses(styles.icon, tone && styles[tone])}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {createElement(iconFor(name, tone === 'highlight'))}
    </span>
  )
}
