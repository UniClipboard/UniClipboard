import { Tooltip } from '@fluentui/react-components'
import type { ReactElement } from 'react'

interface WithTooltipProps {
  content?: string
  relationship: 'description' | 'inaccessible'
  /** Which side of the trigger the tooltip opens on; Fluent's default is above. */
  positioning?: 'above' | 'below' | 'before' | 'after'
  /** Must be a Fluent component: Tooltip merges its trigger props into the child. */
  children: ReactElement
}

export function WithTooltip({ content, relationship, positioning, children }: WithTooltipProps) {
  if (!content) return children
  return (
    <Tooltip
      content={content}
      relationship={relationship}
      positioning={positioning}
      withArrow={false}
    >
      {children}
    </Tooltip>
  )
}
