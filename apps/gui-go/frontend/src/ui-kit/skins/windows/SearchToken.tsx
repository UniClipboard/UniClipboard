import { mergeClasses } from '@fluentui/react-components'
import type { SearchTokenProps } from '@/ui-kit/contract'
import { RemoveButton } from './RemoveButton'
import { useTagStyles } from './tag-styles'

export function SearchToken({ field, value, remove }: SearchTokenProps) {
  const styles = useTagStyles()
  return (
    <span className={mergeClasses(styles.chip, styles.token)}>
      <span className={styles.field}>{field}</span>
      <span className={styles.value}>{value}</span>
      {remove ? <RemoveButton action={remove} /> : null}
    </span>
  )
}
