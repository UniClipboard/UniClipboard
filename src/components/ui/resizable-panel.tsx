import * as ResizablePrimitive from 'react-resizable-panels'

function ResizablePanel({ ...props }: ResizablePrimitive.PanelProps) {
  return <ResizablePrimitive.Panel data-slot="resizable-panel" {...props} />
}

export { ResizablePanel }
