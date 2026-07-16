import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { SoundSettings } from '@/components/setting/general/SoundSettings'
import { playUiSound, setUiSoundEnabled } from '@/lib/ui-sound'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}))

vi.mock('@/lib/ui-sound', () => ({
  playUiSound: vi.fn(),
  readStoredUiSoundEnabled: vi.fn(() => false),
  setUiSoundEnabled: vi.fn(),
}))

describe('SoundSettings', () => {
  it('plays the first toggle cue after enabling sound', async () => {
    const user = userEvent.setup()
    render(<SoundSettings />)

    await user.click(screen.getByRole('switch'))

    expect(setUiSoundEnabled).toHaveBeenCalledWith(true)
    expect(playUiSound).toHaveBeenCalledWith('toggle')
  })
})
