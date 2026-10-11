import { makeStyles, tokens } from '@fluentui/react-components'
import type { ColorDotProps } from '@/ui-kit/contract'
import { swatch } from './theme'

// Board WC3: the 10 px colour dot before a tag name.
const useStyles = makeStyles({
  dot: {
    display: 'inline-block',
    flexShrink: 0,
    width: '0.625rem',
    height: '0.625rem',
    borderRadius: tokens.borderRadiusCircular,
  },
})

/** A colour mark; decorative unless it has a label. */
export function ColorDot({ color, label }: ColorDotProps) {
  const styles = useStyles()
  return (
    <span
      className={styles.dot}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      style={{ backgroundColor: color ? swatch(color) : 'var(--w11-status-neutral)' }}
    />
  )
}
