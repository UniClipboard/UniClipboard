import { ChevronRight } from 'lucide-react'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
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
  return (
    <Collapsible open={open} onOpenChange={onOpenChange} className="flex flex-col">
      <div className="flex items-center justify-between gap-2 px-1 pb-1 pt-4">
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
      <CollapsibleContent className="flex flex-col gap-0.5">{children}</CollapsibleContent>
    </Collapsible>
  )
}

export default HistorySidebarSection
