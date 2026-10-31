import { web } from '@e2e-dev/web'
import { selected } from './suites.mjs'

export const browserEngine = web({
  locale: 'zh-CN',
  reducedMotion: selected.reducedMotion || 'reduce',
  viewport: selected.viewport || { width: 1100, height: 900 },
})
