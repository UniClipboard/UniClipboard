import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

interface HistorySidebarNavItemProps {
  icon: LucideIcon
  label: string
  active?: boolean
  disabled?: boolean
  trailing?: React.ReactNode
  onClick?: () => void
}

function HistorySidebarNavItem({
  icon: Icon,
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
        'flex h-8 w-full items-center gap-2 rounded-md px-2.5 text-ui-body transition-colors',
        active ? 'bg-muted text-foreground font-medium' : 'text-muted-foreground',
        !disabled && !active && 'hover:bg-muted/60 hover:text-foreground',
        disabled && 'cursor-default opacity-60'
      )}
    >
      <Icon className="size-3.5 shrink-0" aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      {trailing}
    </button>
  )
}

export default HistorySidebarNavItem
