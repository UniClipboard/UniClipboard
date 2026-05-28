'use client'

import { Collapsible as CollapsiblePrimitive } from 'radix-ui'

function CollapsibleTrigger({
  ...props
}: React.ComponentProps<typeof CollapsiblePrimitive.CollapsibleTrigger>) {
  return <CollapsiblePrimitive.CollapsibleTrigger data-slot="collapsible-trigger" {...props} />
}

export { CollapsibleTrigger }
