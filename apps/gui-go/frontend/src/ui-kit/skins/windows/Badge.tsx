import { CounterBadge } from '@fluentui/react-components'
import type { BadgeProps } from '@/ui-kit/contract'

export function Badge({ count, label }: BadgeProps) {
  return <CounterBadge count={count} size="small" appearance="filled" aria-label={label} />
}
