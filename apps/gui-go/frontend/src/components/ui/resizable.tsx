import * as ResizablePrimitive from 'react-resizable-panels'
import { cn } from '@/lib/utils'
import { ResizableHandle } from './resizable-handle'
import { ResizablePanel } from './resizable-panel'

function ResizablePanelGroup({ className, ...props }: ResizablePrimitive.GroupProps) {
  return (
    <ResizablePrimitive.Group
      data-slot="resizable-panel-group"
      className={cn('flex h-full w-full aria-[orientation=vertical]:flex-col', className)}
      {...props}
    />
  )
}

export { ResizableHandle, ResizablePanel, ResizablePanelGroup }
