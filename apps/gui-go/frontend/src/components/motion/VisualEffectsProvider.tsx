import { MotionConfig } from 'framer-motion'
import type { ReactNode } from 'react'
import { useVisualEffects } from '@/hooks/useVisualEffects'
import { GuiVisualEffects } from '@/ui-kit/visual-effects'

export default function VisualEffectsProvider({ children }: { children: ReactNode }) {
  const { reduceMotion, lowEffects } = useVisualEffects()
  return (
    <MotionConfig
      reducedMotion={reduceMotion ? 'always' : 'never'}
      skipAnimations={reduceMotion}
      transition={reduceMotion ? { duration: 0, delay: 0 } : undefined}
    >
      <GuiVisualEffects value={{ reduceMotion, lowEffects }}>{children}</GuiVisualEffects>
    </MotionConfig>
  )
}
