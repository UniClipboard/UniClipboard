import type { EffectsSnapshot } from '@host/models'
import { listen } from '@/host/event'
import { commands } from '@/lib/ipc'

export const visualEffectsApi = {
  get: () => commands.getVisualEffects(),
  setMode: commands.setVisualEffectsMode,
  environment: commands.reportVisualEffectsEnvironment,
  beginSample: commands.beginVisualEffectsSample,
  reportSample: commands.reportVisualEffectsSample,
  subscribe: (listener: (snapshot: EffectsSnapshot) => void) =>
    listen<EffectsSnapshot>('visual-effects://changed', event => listener(event.payload)),
}
