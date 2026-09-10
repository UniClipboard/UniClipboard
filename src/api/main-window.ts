import { commands } from '@/lib/ipc'

export async function reportMainWindowPresentationReady(): Promise<void> {
  const generation = (window as Window & { __UC_MAIN_WINDOW_GENERATION__?: string })
    .__UC_MAIN_WINDOW_GENERATION__
  if (typeof generation !== 'string') return
  await commands.mainWindowPresentationReady(generation)
}
