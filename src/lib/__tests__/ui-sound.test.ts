import { play } from 'cuelume'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { playUiSound } from '@/lib/ui-sound'

vi.mock('cuelume', () => ({
  bind: vi.fn(),
  play: vi.fn(),
  setEnabled: vi.fn(),
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
