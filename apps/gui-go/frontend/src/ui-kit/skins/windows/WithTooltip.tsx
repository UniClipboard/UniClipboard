import { Tooltip } from '@fluentui/react-components'
import type { ReactElement } from 'react'

interface WithTooltipProps {
  content?: string
  relationship: 'description' | 'inaccessible'
  /** Must be a Fluent component: Tooltip merges its trigger props into the child. */
  children: ReactElement
}

export function WithTooltip({ content, relationship, children }: WithTooltipProps) {
  if (!content) return children
  return (
    <Tooltip content={content} relationship={relationship} withArrow={false}>
      {children}
    </Tooltip>
  )
}
