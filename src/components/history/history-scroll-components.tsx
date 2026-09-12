import { ScrollArea } from '@base-ui/react/scroll-area'
import type { ListProps, ScrollerProps } from 'react-virtuoso'
import { ScrollBar } from '@/components/ui/scroll-area'

// Keep the virtual list ref on the actual scrolling viewport.
export function HistoryScroller({ ref, children, style, ...props }: ScrollerProps) {
  return (
    <ScrollArea.Root className="relative h-full min-h-0">
      <ScrollArea.Viewport ref={ref} style={style} {...props}>
        {children}
      </ScrollArea.Viewport>
      <ScrollBar />
    </ScrollArea.Root>
  )
}

// Recompute the thumb when virtual list padding or content size changes.
export function HistoryList(props: ListProps) {
  return <ScrollArea.Content {...props} style={{ ...props.style, width: '100%', minWidth: 0 }} />
}
