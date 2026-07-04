import React from 'react'

/**
 * One label/value row in a detail panel's facts list (hairline-separated,
 * micro uppercase label on the left). Shared by the local/peer/mobile
 * device panels so their profile sections read identically.
 */
const PanelFactRow: React.FC<{ label: string; children: React.ReactNode }> = ({
  label,
  children,
}) => (
  <div className="flex items-center gap-4 border-b border-border/40 py-2.5 last:border-b-0">
    <span className="w-28 shrink-0 text-[9.5px] font-semibold uppercase tracking-[0.12em] text-muted-foreground/70">
      {label}
    </span>
    <div className="flex min-w-0 flex-1 items-center gap-2 text-foreground">{children}</div>
  </div>
)

export default PanelFactRow
