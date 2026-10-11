import { mergeClasses } from '@fluentui/react-components'
import type { TagProps } from '@/ui-kit/contract'
import { RemoveButton } from './RemoveButton'
import { useTagStyles } from './tag-styles'

/** Project tag chip. Fluent's dismissible Tag makes the whole chip one remove button, so the
 * remove action is a separate button and the name stays plain text. */
export function Tag({ name, color = 'neutral', remove }: TagProps) {
  const styles = useTagStyles()
  return (
    <span className={mergeClasses(styles.chip, styles[color])}>
      <span className={styles.name}>{name}</span>
      {remove ? <RemoveButton action={remove} /> : null}
    </span>
  )
}
