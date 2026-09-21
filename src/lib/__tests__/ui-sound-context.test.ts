import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  installIdleSuspendingAudioContext,
  suspendUiAudioContexts,
  UI_AUDIO_IDLE_SUSPEND_MS,
} from '@/lib/ui-sound-context'

class FakeAudioContext {
  static instances: FakeAudioContext[] = []

  state: AudioContextState = 'running'
  readonly options: AudioContextOptions | undefined
  readonly suspend = vi.fn(async () => {
    this.state = 'suspended'
  })
  readonly resume = vi.fn(async () => {
    this.state = 'running'
  })

  constructor(options?: AudioContextOptions) {
    this.options = options
    FakeAudioContext.instances.push(this)
  }

  createGain(): GainNode {
    return {} as GainNode
  }

  async close(): Promise<void> {
    this.state = 'closed'
  }
}

type AudioWindow = Window & { AudioContext?: unknown }

const createContext = (options?: AudioContextOptions): FakeAudioContext => {
  const Ctor = (window as AudioWindow).AudioContext as new (
    options?: AudioContextOptions
  ) => FakeAudioContext
  return new Ctor(options)
}

describe('installIdleSuspendingAudioContext', () => {
  let dispose: () => void
  let originalAudioContext: unknown

  beforeEach(() => {
    vi.useFakeTimers()
    FakeAudioContext.instances = []
    originalAudioContext = (window as AudioWindow).AudioContext
    ;(window as AudioWindow).AudioContext = FakeAudioContext
    dispose = installIdleSuspendingAudioContext()
  })

  afterEach(() => {
    dispose()
    ;(window as AudioWindow).AudioContext = originalAudioContext
    vi.useRealTimers()
  })

  it('passes constructor options through unchanged', () => {
    expect(createContext().options).toBeUndefined()
    expect(createContext({ sampleRate: 44100 }).options).toEqual({ sampleRate: 44100 })
  })

  it('suspends the context once no cue has started for the idle period', async () => {
    const context = createContext()

    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS - 1)
    expect(context.suspend).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(context.suspend).toHaveBeenCalledTimes(1)
    expect(context.state).toBe('suspended')
  })

  it('keeps the context running while cues keep starting', async () => {
    const context = createContext()

    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS - 1)
    context.createGain()
    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS - 1)

    expect(context.suspend).not.toHaveBeenCalled()
  })

  it('suspends again after a cue resumed the context', async () => {
    const context = createContext()
    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS)

    // Cuelume resumes a suspended context itself before rendering a cue.
    await context.resume()
    context.createGain()
    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS)

    expect(context.suspend).toHaveBeenCalledTimes(2)
  })

  it('resumes when a cue starts while an idle suspend is still settling', async () => {
    const context = createContext()
    let settleSuspend = (): void => {}
    context.suspend.mockImplementationOnce(
      () =>
        new Promise<void>(resolve => {
          settleSuspend = resolve
        })
    )

    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS)
    context.createGain()
    settleSuspend()

    expect(context.resume).toHaveBeenCalledTimes(1)
  })

  it('suspends immediately on request', () => {
    const context = createContext()

    suspendUiAudioContexts()

    expect(context.suspend).toHaveBeenCalledTimes(1)
  })

  it('does not suspend a context that is not running', async () => {
    const context = createContext()
    context.state = 'suspended'

    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS)

    expect(context.suspend).not.toHaveBeenCalled()
  })

  it('stops tracking a closed context', async () => {
    const context = createContext()

    await context.close()
    await vi.advanceTimersByTimeAsync(UI_AUDIO_IDLE_SUSPEND_MS)
    suspendUiAudioContexts()

    expect(context.suspend).not.toHaveBeenCalled()
  })

  it('restores the native constructor and suspends on dispose', () => {
    const context = createContext()
    expect((window as AudioWindow).AudioContext).not.toBe(FakeAudioContext)

    dispose()

    expect((window as AudioWindow).AudioContext).toBe(FakeAudioContext)
    expect(context.suspend).toHaveBeenCalledTimes(1)
  })

  it('stays installed until every caller has disposed', () => {
    const patched = (window as AudioWindow).AudioContext
    const disposeSecond = installIdleSuspendingAudioContext()
    expect((window as AudioWindow).AudioContext).toBe(patched)

    dispose()
    expect((window as AudioWindow).AudioContext).toBe(patched)

    disposeSecond()
    expect((window as AudioWindow).AudioContext).toBe(FakeAudioContext)
  })

  it('ignores repeated calls to the same disposer', () => {
    const patched = (window as AudioWindow).AudioContext
    const disposeSecond = installIdleSuspendingAudioContext()

    dispose()
    dispose()

    expect((window as AudioWindow).AudioContext).toBe(patched)
    disposeSecond()
  })
})

describe('installIdleSuspendingAudioContext without Web Audio', () => {
  it('is a no-op', () => {
    const audioWindow = window as AudioWindow & { webkitAudioContext?: unknown }
    const original = audioWindow.AudioContext
    const originalWebkit = audioWindow.webkitAudioContext
    delete audioWindow.AudioContext
    delete audioWindow.webkitAudioContext

    expect(() => installIdleSuspendingAudioContext()()).not.toThrow()

    audioWindow.AudioContext = original
    audioWindow.webkitAudioContext = originalWebkit
  })
})
