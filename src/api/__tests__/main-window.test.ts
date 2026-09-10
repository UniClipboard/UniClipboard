import { afterEach, describe, expect, it, vi } from 'vitest'
import { reportMainWindowPresentationReady } from '@/api/main-window'

const report = vi.hoisted(() => vi.fn().mockResolvedValue(undefined))
vi.mock('@/lib/ipc', () => ({ commands: { mainWindowPresentationReady: report } }))
afterEach(() => {
  delete (window as Window & { __UC_MAIN_WINDOW_GENERATION__?: string })
    .__UC_MAIN_WINDOW_GENERATION__
  report.mockClear()
})
describe('main document readiness', () => {
  it('does nothing in the browser without a native window generation', async () => {
    await reportMainWindowPresentationReady()
    expect(report).not.toHaveBeenCalled()
  })
  it('reports the generation embedded in this document', async () => {
    Object.defineProperty(window, '__UC_MAIN_WINDOW_GENERATION__', {
      value: '42',
      configurable: true,
    })
    await reportMainWindowPresentationReady()
    expect(report).toHaveBeenCalledWith('42')
  })
})
