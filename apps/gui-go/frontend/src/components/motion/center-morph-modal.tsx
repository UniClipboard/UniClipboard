import type { ReactNode } from 'react'
import '@/components/motion/center-morph-modal.css'

/** Keep the shadow outside the clipped surface so it follows the unfolding edge. */
export function CenterMorphModalSurface({ children }: { children: ReactNode }) {
  return <div className="center-morph-layer">{children}</div>
}
