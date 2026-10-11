import { mergeClasses, Tag as FluentTag, TagGroup } from '@fluentui/react-components'
import type { TagProps } from '@/ui-kit/contract'
import { useTagStyles } from './tag-styles'

export function Tag({ name, color = 'neutral', remove }: TagProps) {
  const styles = useTagStyles()
  const tag = (
    <FluentTag
      size="extra-small"
      shape="rounded"
      value={name}
      dismissible={Boolean(remove)}
      dismissIcon={remove ? { 'aria-label': remove.label } : undefined}
      className={mergeClasses(styles.base, styles[color])}
    >
      {name}
    </FluentTag>
  )
  if (!remove) return tag
  // Fluent reports dismissal (click or Delete/Backspace on the tag) through the group.
  return (
    <TagGroup aria-label={name} onDismiss={() => remove.onClick()}>
      {tag}
    </TagGroup>
  )
}
