import { Plus } from 'lucide-react'
import React from 'react'
import { cn } from '@/lib/utils'

const EmptyAddRow: React.FC<{
  label: string
  onClick: () => void
  dimmed?: boolean
}> = ({ label, onClick, dimmed }) => (
  <button
    type="button"
    onClick={onClick}
    className={cn(
      'flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-left text-ui-body text-muted-foreground/70 transition-colors hover:bg-muted/60 hover:text-foreground',
      dimmed && 'opacity-60 hover:bg-transparent hover:text-muted-foreground/70'
    )}
  >
    <Plus className="size-3" />
    {label}
  </button>
)

export default EmptyAddRow
