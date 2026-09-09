import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import QuickPanelScaleIndicator from '../components/QuickPanelScaleIndicator'
import type { QuickPanelScaleFeedback } from '../window-layout'

const { install, dispose } = vi.hoisted(() => ({ install: vi.fn(), dispose: vi.fn() }))
vi.mock('../window-layout', () => ({ installWindowResizeShortcuts: install }))

beforeEach(async () => {
  await i18n.changeLanguage('zh-CN')
  vi.useFakeTimers()
  install.mockReset().mockReturnValue(dispose)
  dispose.mockClear()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

function adjust(feedback: QuickPanelScaleFeedback) {
  act(() => install.mock.calls[0][0](feedback))
}

it('shows both percentages and extends visibility after repeated adjustments', () => {
  render(<QuickPanelScaleIndicator />)
  expect(screen.queryByRole('status')).toBeNull()
  adjust({ textPercent: 125, windowPercent: 110 })
  expect(screen.getByRole('status')).toHaveTextContent('文字125%窗口110%')
  act(() => vi.advanceTimersByTime(1000))
  adjust({ textPercent: 150, windowPercent: 110 })
  act(() => vi.advanceTimersByTime(1000))
  expect(screen.getByRole('status')).toHaveTextContent('文字150%窗口110%')
  act(() => vi.advanceTimersByTime(200))
  expect(screen.queryByRole('status')).toBeNull()
})

it('cleans up keyboard handling and the pending dismissal on unmount', () => {
  const { unmount } = render(<QuickPanelScaleIndicator />)
  adjust({ textPercent: 100, windowPercent: 100 })
  unmount()
  expect(dispose).toHaveBeenCalledOnce()
  expect(vi.getTimerCount()).toBe(0)
})

it.each([
  ['zh-CN', '文字', '窗口'],
  ['zh-TW', '文字', '視窗'],
  ['en-US', 'Text', 'Window'],
  ['ja-JP', '文字', 'ウィンドウ'],
  ['ru-RU', 'Текст', 'Окно'],
  ['pt-BR', 'Texto', 'Janela'],
])('renders the real %s translations', async (language, textLabel, windowLabel) => {
  await i18n.changeLanguage(language)
  render(<QuickPanelScaleIndicator />)
  adjust({ textPercent: 125, windowPercent: 110 })
  expect(screen.getByRole('status')).toHaveTextContent(`${textLabel}125%${windowLabel}110%`)
})
