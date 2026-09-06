import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import { submitDiagnosticFeedback } from '@/observability/diagnostics'
import { FeedbackDialog } from '../FeedbackDialog'

vi.mock('@/observability/diagnostics', async importOriginal => ({
  ...(await importOriginal<typeof import('@/observability/diagnostics')>()),
  submitDiagnosticFeedback: vi.fn(),
}))

describe('feedback delivery', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    localStorage.removeItem('uniclipboard.feedback.email')
    await i18n.changeLanguage('zh-CN')
  })

  it('waits for the provider before closing and preserves the submitted content', async () => {
    let finish!: () => void
    vi.mocked(submitDiagnosticFeedback).mockReturnValue(
      new Promise(resolve => {
        finish = resolve
      })
    )
    const onOpenChange = vi.fn()
    render(<FeedbackDialog open onOpenChange={onOpenChange} />)
    fireEvent.change(screen.getByRole('textbox', { name: '反馈内容' }), {
      target: { value: 'The window did not open.' },
    })
    fireEvent.change(screen.getByRole('textbox', { name: '电子邮箱（选填）' }), {
      target: { value: 'developer@example.test' },
    })
    fireEvent.click(screen.getByRole('button', { name: '提交反馈' }))
    expect(submitDiagnosticFeedback).toHaveBeenCalledWith({
      message: 'The window did not open.',
      email: 'developer@example.test',
    })
    expect(screen.getByRole('button', { name: '提交反馈' })).toBeDisabled()
    expect(onOpenChange).not.toHaveBeenCalled()
    await act(async () => finish())
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
  })

  it('keeps the form and user text available if the provider rejects submission', async () => {
    vi.mocked(submitDiagnosticFeedback).mockRejectedValue(new Error('provider unavailable'))
    const onOpenChange = vi.fn()
    render(<FeedbackDialog open onOpenChange={onOpenChange} />)
    const content = screen.getByRole('textbox', { name: '反馈内容' })
    fireEvent.change(content, { target: { value: 'Keep this feedback.' } })
    fireEvent.click(screen.getByRole('button', { name: '提交反馈' }))
    await waitFor(() => expect(screen.getByRole('button', { name: '提交反馈' })).toBeEnabled())
    expect(content).toHaveValue('Keep this feedback.')
    expect(onOpenChange).not.toHaveBeenCalled()
  })
})
