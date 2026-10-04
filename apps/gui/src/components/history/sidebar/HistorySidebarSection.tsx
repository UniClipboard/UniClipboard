import { ChevronRight } from 'lucide-react'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { useSidebarSlot } from '@/contexts/sidebar-slot-context'
import { cn } from '@/lib/utils'

interface HistorySidebarSectionProps {
  label: string
  open: boolean
  onOpenChange: (open: boolean) => void
  trailing?: React.ReactNode
  children: React.ReactNode
}

function HistorySidebarSection({
  label,
  open,
  onOpenChange,
  trailing,
  children,
}: HistorySidebarSectionProps) {
  const { libraryOwnsNavigation: windowEdge } = useSidebarSlot()
  return (
    <Collapsible open={open} onOpenChange={onOpenChange} className="flex flex-col">
      <div
        className={cn(
          'flex items-center justify-between gap-2',
          windowEdge ? 'mt-3 h-7.5 pl-2.5 pr-1' : 'px-1 pb-1 pt-4'
        )}
      >
        <CollapsibleTrigger
          className="flex min-w-0 flex-1 items-center gap-1 text-left text-ui-caption font-semibold uppercase text-muted-foreground/80"
          type="button"
        >
          <ChevronRight
            aria-hidden="true"
            className={cn('size-3 shrink-0 transition-transform', open && 'rotate-90')}
          />
          <span className="min-w-0 truncate">{label}</span>
        </CollapsibleTrigger>
        {trailing}
      </div>
      <CollapsibleContent className={cn('flex flex-col', windowEdge ? 'gap-px' : 'gap-0.5')}>
        {children}
      </CollapsibleContent>
    </Collapsible>
  )
}

export default HistorySidebarSection
