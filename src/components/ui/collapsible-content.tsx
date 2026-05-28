'use client'

import { Collapsible as CollapsiblePrimitive } from 'radix-ui'

function CollapsibleContent({
  ...props
}: React.ComponentProps<typeof CollapsiblePrimitive.CollapsibleContent>) {
  return <CollapsiblePrimitive.CollapsibleContent data-slot="collapsible-content" {...props} />
}

export { CollapsibleContent }
