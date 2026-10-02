import { bind, play } from 'cuelume'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { initializeUiSound, playUiSound, setUiSoundEnabled } from '@/lib/ui-sound'
import { installIdleSuspendingAudioContext, suspendUiAudioContexts } from '@/lib/ui-sound-context'

vi.mock('cuelume', () => ({
  bind: vi.fn(),
  play: vi.fn(),
  setEnabled: vi.fn(),
}))

vi.mock('@/lib/ui-sound-context', () => ({
  installIdleSuspendingAudioContext: vi.fn(() => vi.fn()),
  suspendUiAudioContexts: vi.fn(),
}))

describe('playUiSound', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('forwards the requested cue to the audio engine', () => {
    playUiSound('success')

    expect(play).toHaveBeenCalledWith('success')
  })

  // Callers invoke playUiSound inside the success path of clipboard/paste
  // flows, so a throwing `play()` would surface as a failed operation.
  it('swallows playback errors so callers never see a failed operation', () => {
    vi.mocked(play).mockImplementationOnce(() => {
      throw new Error('InvalidStateError: AudioContext is closed')
    })

    expect(() => playUiSound('success')).not.toThrow()
  })
})

// A running AudioContext keeps the WebView's output stream open at a small IO
// buffer even while silent, which degrades audio system-wide (#1715).
describe('audio hardware release', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    window.localStorage.clear()
  })

  it('installs the idle-suspending audio context before binding cues', () => {
    const dispose = initializeUiSound()

    expect(installIdleSuspendingAudioContext).toHaveBeenCalledTimes(1)
    expect(vi.mocked(installIdleSuspendingAudioContext).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(bind).mock.invocationCallOrder[0]
    )

    dispose()
  })

  it('restores the native audio context on dispose', () => {
    const disposeAudioContext = vi.fn()
    vi.mocked(installIdleSuspendingAudioContext).mockReturnValueOnce(disposeAudioContext)

    initializeUiSound()()

    expect(disposeAudioContext).toHaveBeenCalledTimes(1)
  })

  it('releases the audio hardware as soon as sounds are turned off', () => {
    setUiSoundEnabled(false)

    expect(suspendUiAudioContexts).toHaveBeenCalledTimes(1)
  })

  it('does not suspend when sounds are turned on', () => {
    setUiSoundEnabled(true)

    expect(suspendUiAudioContexts).not.toHaveBeenCalled()
  })
})
