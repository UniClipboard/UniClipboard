import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SmoothModeSetting from '@/components/setting/SmoothModeSetting'
const mock = vi.hoisted(() => ({ setMode: vi.fn(), unavailable: false, persistence: 'saved' }))
vi.mock('@/hooks/useVisualEffects', () => ({
  useVisualEffects: () => ({
    sessionId: 'gui',
    mode: 'auto',
    lowEffects: true,
    reason: 'platform_default',
    persistence: mock.persistence,
  }),
  useVisualEffectsUnavailable: () => mock.unavailable,
}))
vi.mock('@/lib/visual-effects-store', () => ({ visualEffectsStore: { setMode: mock.setMode } }))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))

describe('SmoothModeSetting', () => {
  beforeEach(() => {
    mock.setMode.mockReset()
    mock.persistence = 'saved'
    mock.unavailable = false
  })
  it('offers three radio choices and saves a selection once', async () => {
    mock.setMode.mockResolvedValue(undefined)
    render(<SmoothModeSetting />)
    expect(screen.getAllByRole('radio')).toHaveLength(3)
    expect(screen.getByRole('radio', { name: 'smoothMode.auto' })).toBeChecked()
    fireEvent.click(screen.getByRole('radio', { name: 'smoothMode.effects' }))
    await waitFor(() => expect(mock.setMode).toHaveBeenCalledExactlyOnceWith('effects'))
  })
  it('explains when a selection could not be saved', () => {
    mock.persistence = 'session_only'
    render(<SmoothModeSetting />)
    expect(screen.getByRole('alert')).toHaveTextContent('smoothMode.notSaved')
  })
})
