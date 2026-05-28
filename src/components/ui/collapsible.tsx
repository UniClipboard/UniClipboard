'use client'

import { Collapsible as CollapsiblePrimitive } from 'radix-ui'
import { CollapsibleContent } from './collapsible-content'
import { CollapsibleTrigger } from './collapsible-trigger'

function Collapsible({ ...props }: React.ComponentProps<typeof CollapsiblePrimitive.Root>) {
  return <CollapsiblePrimitive.Root data-slot="collapsible" {...props} />
}

export { Collapsible, CollapsibleTrigger, CollapsibleContent }
