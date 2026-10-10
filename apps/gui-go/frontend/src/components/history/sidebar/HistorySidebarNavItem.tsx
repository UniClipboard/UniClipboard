import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

type HistorySidebarNavItemProps = (
  | { icon: LucideIcon; leading?: never }
  // A custom 14px leading mark (e.g. a tag's colour dot) in place of an icon.
  | { icon?: never; leading: React.ReactNode }
) & {
  label: string
  active?: boolean
  disabled?: boolean
  trailing?: React.ReactNode
  onClick?: () => void
}

function HistorySidebarNavItem({
  icon: Icon,
  leading,
  label,
  active = false,
  disabled = false,
  trailing,
  onClick,
}: HistorySidebarNavItemProps) {
  return (
    <button
      type="button"
      aria-current={active ? 'true' : undefined}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        // An inset ring: an outer one would spill past the sidebar's edge.
        'flex w-full items-center text-ui-body transition-colors outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50',
        // Window-edge sidebar (HSidebar.dc.html): 30px rows, full-contrast
        // labels, accent background for the current row.
        'h-7.5 gap-2.5 rounded-lg px-2.5 text-sidebar-foreground',
        active ? 'bg-foreground/8 font-medium' : !disabled && 'hover:bg-foreground/5',
        disabled && 'cursor-default opacity-60'
      )}
    >
      {Icon ? (
        <Icon className="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
      ) : (
        <span aria-hidden="true" className="flex size-3.5 shrink-0 items-center justify-center">
          {leading}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {trailing}
    </button>
  )
}

export default HistorySidebarNavItem
