import { useEffect, useEffectEvent } from 'react'
import { startVisualEffectsSampler } from '@/lib/visual-effects-sampler'

export function useVisualEffectsSampling(ready: boolean) {
  const isReady = useEffectEvent(() => ready)
  useEffect(() => {
    if (!ready) return
    return startVisualEffectsSampler(isReady)
  }, [ready])
}
