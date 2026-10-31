import { web } from '@e2e-dev/web'

export const backendEngine = web({
  locale: 'en-US',
  reducedMotion: 'reduce',
  viewport: { width: 1280, height: 800 },
})
