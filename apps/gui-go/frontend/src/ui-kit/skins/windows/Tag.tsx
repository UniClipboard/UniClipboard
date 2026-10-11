import { mergeClasses } from '@fluentui/react-components'
import type { TagPaletteColor, TagProps } from '@/ui-kit/contract'
import { RemoveButton } from './RemoveButton'
import { useTagStyles } from './tag-styles'

/**
 * Project tag chip. Fluent's dismissible Tag makes the whole chip one remove button, so the
 * remove action is a separate button and the name stays plain text. Board WC2: a tag only some of
 * the selected items carry has a dashed outline and its count ("2 of 3"); with `action` the chip
 * itself is a button (e.g. add the tag to all of them).
 */
export function Tag({ name, color, remove, detail, partial, action }: TagProps) {
  const styles = useTagStyles()
  const custom = color?.startsWith('#') ? color : undefined
  const palette = color && !custom ? styles[color as TagPaletteColor] : styles.neutral
  const className = mergeClasses(styles.chip, palette, partial && styles.partial)
  // 8-digit hex: the tint of a custom colour, without color-mix().
  const style = custom ? { backgroundColor: `${custom}29`, color: custom } : undefined
  const label = (
    <>
      <span className={styles.name}>{name}</span>
      {detail ? <span className={styles.detail}>{detail}</span> : null}
    </>
  )
  return (
    <span className={className} style={style}>
      {action ? (
        <button
          type="button"
          className={styles.action}
          aria-label={action.label}
          onClick={() => action.onClick()}
        >
          {label}
        </button>
      ) : (
        label
      )}
      {remove ? <RemoveButton action={remove} /> : null}
    </span>
  )
}
