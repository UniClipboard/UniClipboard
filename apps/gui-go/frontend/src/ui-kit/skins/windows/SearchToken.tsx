import { mergeClasses, Tag as FluentTag, TagGroup } from '@fluentui/react-components'
import type { SearchTokenProps } from '@/ui-kit/contract'
import { useTagStyles } from './tag-styles'

export function SearchToken({ field, value, onRemove }: SearchTokenProps) {
  const styles = useTagStyles()
  const name = `${field} ${value}`
  const tag = (
    <FluentTag
      size="small"
      appearance="outline"
      value={name}
      dismissible={Boolean(onRemove)}
      dismissIcon={onRemove ? { 'aria-label': `Remove ${name}` } : undefined}
      className={mergeClasses(styles.token)}
    >
      <span className={styles.field}>{field}</span>
      <span className={styles.value}>{value}</span>
    </FluentTag>
  )
  if (!onRemove) return tag
  return (
    <TagGroup aria-label={name} onDismiss={() => onRemove()}>
      {tag}
    </TagGroup>
  )
}
