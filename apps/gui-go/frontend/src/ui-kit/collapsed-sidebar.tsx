import type { ReactNode } from 'react'

/** Shared fallback for skins without a compact navigation rail. */
export function CollapsedSidebar({
  hidden,
  overlay,
  blocked,
  children,
}: {
  hidden?: boolean
  overlay: boolean
  blocked: boolean
  children?: ReactNode
}) {
  if (!children || (!hidden && !overlay)) return null
  return (
    <aside
      className="flex w-12 shrink-0 flex-col justify-end border-r border-border pb-3"
      inert={blocked}
      aria-hidden={blocked || undefined}
    >
      {children}
    </aside>
  )
}
