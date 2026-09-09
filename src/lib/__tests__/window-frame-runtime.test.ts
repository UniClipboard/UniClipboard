import { afterEach, expect, it, vi } from 'vitest'
import { WINDOW_FRAME_STORAGE_KEY } from '@/lib/window-frame'
import { initializeWindowFrame } from '@/lib/window-frame-runtime'

const mocks = vi.hoisted(() => ({ setDecorations: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => mocks }))
vi.mock('@/lib/platform', () => ({
  detectPlatformInfo: () => ({
    isLinux: true,
    isTauri: true,
    isMac: false,
    isWindows: false,
  }),
}))
vi.mock('@/lib/logger', () => ({ createLogger: () => ({ error: vi.fn() }) }))
afterEach(() => {
  localStorage.clear()
  delete window.__UC_WINDOW_FRAME_DEFAULT__
  vi.clearAllMocks()
})

it('applies the automatic desktop default on every window opening', async () => {
  window.__UC_WINDOW_FRAME_DEFAULT__ = 'none'
  await initializeWindowFrame()
  await initializeWindowFrame()
  expect(mocks.setDecorations.mock.calls).toEqual([[false], [false]])
})

it('restores an explicit system frame even on a tiling desktop', async () => {
  window.__UC_WINDOW_FRAME_DEFAULT__ = 'none'
  localStorage.setItem(WINDOW_FRAME_STORAGE_KEY, 'true')
  await initializeWindowFrame()
  expect(mocks.setDecorations).toHaveBeenCalledWith(true)
})

it('waits for native decoration changes before startup continues', async () => {
  let finish!: () => void
  mocks.setDecorations.mockImplementationOnce(
    () =>
      new Promise<void>(resolve => {
        finish = resolve
      })
  )
  const done = vi.fn()
  const ready = initializeWindowFrame().then(done)
  await Promise.resolve()
  expect(done).not.toHaveBeenCalled()
  finish()
  await ready
  expect(done).toHaveBeenCalledOnce()
})
