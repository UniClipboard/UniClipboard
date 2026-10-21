import type { EffectsSnapshot } from '@host/models'
import { listen } from '@/host/event'
import { commands } from '@/lib/ipc'

export const visualEffectsApi = {
  get: () => commands.getVisualEffects(),
  setMode: commands.setVisualEffectsMode,
  environment: commands.reportVisualEffectsEnvironment,
  beginSample: commands.beginVisualEffectsSample,
  reportSample: commands.reportVisualEffectsSample,
  async subscribeLifecycle(listener: () => void): Promise<() => void> {
    const registrations = await Promise.allSettled(
      [
        'common:WindowHide',
        'common:WindowMinimise',
        'common:WindowShow',
        'common:WindowUnMinimise',
        'common:SystemWillSleep',
        'common:SystemDidWake',
      ].map(event => listen(event, listener))
    )
    const dispose = () =>
      registrations.forEach(result => {
        if (result.status === 'fulfilled') result.value()
      })
    const failure = registrations.find(result => result.status === 'rejected')
    if (failure?.status === 'rejected') {
      dispose()
      throw failure.reason
    }
    return dispose
  },
  subscribe: (listener: (snapshot: EffectsSnapshot) => void) =>
    listen<EffectsSnapshot>('visual-effects://changed', event => listener(event.payload)),
}
