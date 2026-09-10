import { act, cleanup, render, screen, fireEvent } from '@testing-library/react'
import { afterEach, expect, it, vi } from 'vitest'
import { UpgradePreview } from './UpgradePreview'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ i18n: { language: 'en-US', changeLanguage: vi.fn() } }),
}))
vi.mock('@/components/app/StartupProgressScreen', () => ({
  StartupProgressScreen: ({
    snapshot,
  }: {
    snapshot: { upgrade: { required: boolean }; state: string }
  }) => <output>{`${snapshot.state}:${snapshot.upgrade.required}`}</output>,
}))
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  window.history.replaceState({}, '', '/')
})
it.each([
  ['cold-start', false],
  ['upgrading', true],
])('preserves the upgrade requirement throughout %s playback', (scenario, required) => {
  vi.useFakeTimers()
  window.history.replaceState({}, '', `/?scenario=${scenario}`)
  render(<UpgradePreview />)
  fireEvent.click(screen.getByRole('button', { name: 'Play' }))
  act(() => vi.advanceTimersByTime(20_000))
  expect(screen.getByRole('status').textContent).toBe(`starting_services:${required}`)
  act(() => vi.advanceTimersByTime(3_000))
  expect(screen.getByRole('status').textContent).toBe(`ready:${required}`)
})
