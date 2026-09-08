import { MotionConfig } from 'framer-motion'
import type { ReactNode } from 'react'
import { useVisualEffects } from '@/hooks/useVisualEffects'

export default function VisualEffectsProvider({ children }: { children: ReactNode }) {
  const { reduceMotion } = useVisualEffects()
  return (
    <MotionConfig
      reducedMotion={reduceMotion ? 'always' : 'never'}
      skipAnimations={reduceMotion}
      transition={reduceMotion ? { duration: 0, delay: 0 } : undefined}
    >
      {children}
    </MotionConfig>
  )
}
