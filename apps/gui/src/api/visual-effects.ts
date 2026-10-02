import { listen } from '@tauri-apps/api/event'
import { commands } from '@/lib/ipc'
import type { EffectsSnapshot } from '@/lib/ipc-bindings.generated'

export const visualEffectsApi = {
  get: () => commands.getVisualEffects(),
  setMode: commands.setVisualEffectsMode,
  environment: commands.reportVisualEffectsEnvironment,
  beginSample: commands.beginVisualEffectsSample,
  reportSample: commands.reportVisualEffectsSample,
  subscribe: (listener: (snapshot: EffectsSnapshot) => void) =>
    listen<EffectsSnapshot>('visual-effects://changed', event => listener(event.payload)),
}
