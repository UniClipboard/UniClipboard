import type { ReactNode } from 'react'
import { useGroupedListStyles } from './grouped-list-styles'

/** Virtuoso footer; its content arrives through Virtuoso's `context`. */
export function GroupedListFooter({ context }: { context: { footer?: ReactNode } }) {
  const styles = useGroupedListStyles()
  return context.footer ? <div className={styles.footer}>{context.footer}</div> : null
}
