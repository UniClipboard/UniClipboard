import { AnimatePresence, m } from 'framer-motion'
import type { ReactNode } from 'react'
import { useReducedMotion } from '@/hooks/useVisualEffects'

const VIEW_TRANSITION = { duration: 0.18, ease: [0.22, 1, 0.36, 1] } as const

export function AppViewTransition({ viewKey, children }: { viewKey: string; children: ReactNode }) {
  const reduceMotion = useReducedMotion()
  const disabled = reduceMotion

  return (
    <div className="relative h-full w-full overflow-hidden">
      <AnimatePresence initial={false} mode="sync">
        <m.div
          key={viewKey}
          initial={false}
          animate={{ opacity: 1, y: 0 }}
          exit={undefined}
          transition={disabled ? { duration: 0 } : VIEW_TRANSITION}
          className="absolute inset-0 flex min-h-0"
        >
          {children}
        </m.div>
      </AnimatePresence>
    </div>
  )
}
