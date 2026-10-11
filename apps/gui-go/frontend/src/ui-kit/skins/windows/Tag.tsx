import { mergeClasses } from '@fluentui/react-components'
import type { TagPaletteColor, TagProps } from '@/ui-kit/contract'
import { RemoveButton } from './RemoveButton'
import { useTagStyles } from './tag-styles'

/** Project tag chip. Fluent's dismissible Tag makes the whole chip one remove button, so the
 * remove action is a separate button and the name stays plain text. */
export function Tag({ name, color, remove }: TagProps) {
  const styles = useTagStyles()
  const custom = color?.startsWith('#') ? color : undefined
  const palette = color && !custom ? styles[color as TagPaletteColor] : styles.neutral
  return (
    <span
      className={mergeClasses(styles.chip, palette)}
      // 8-digit hex: the tint of a custom colour, without color-mix().
      style={custom ? { backgroundColor: `${custom}29`, color: custom } : undefined}
    >
      <span className={styles.name}>{name}</span>
      {remove ? <RemoveButton action={remove} /> : null}
    </span>
  )
}
