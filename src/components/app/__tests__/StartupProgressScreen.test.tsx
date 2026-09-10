import { act, fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { StartupProgressScreen } from '@/components/app/StartupProgressScreen'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'

describe('startup and upgrade progress', () => {
  it('does not present an ordinary cold start as an upgrade', () => {
    const snapshot = makeUpgradePreview('starting', 20)
    snapshot.upgrade!.required = false
    render(<StartupProgressScreen snapshot={snapshot} onRetry={vi.fn()} onExport={vi.fn()} />)
    expect(screen.queryByRole('progressbar')).not.toBeInTheDocument()
    expect(screen.queryByText(/资料已升级|Data upgraded|处理记录|Activity/)).not.toBeInTheDocument()
    expect(screen.getByRole('heading')).toHaveTextContent(/正在启动|Starting the app/)
  })
  it('keeps elapsed time moving without any backend progress update', () => {
    vi.useFakeTimers()
    const view = render(
      <StartupProgressScreen
        snapshot={makeUpgradePreview('upgrading', 20)}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    try {
      expect(screen.getByText(/0:20/)).toBeInTheDocument()
      act(() => vi.advanceTimersByTime(3000))
      expect(screen.getByText(/0:23/)).toBeInTheDocument()
    } finally {
      view.unmount()
      vi.useRealTimers()
    }
  })

  it('does not show 100 percent while finishing a step or verifying data', () => {
    const snapshot = makeUpgradePreview('upgrading', 40)
    const view = render(
      <StartupProgressScreen snapshot={snapshot} onRetry={vi.fn()} onExport={vi.fn()} />
    )
    expect(screen.queryByText(/100%/)).not.toBeInTheDocument()
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow')
    view.rerender(
      <StartupProgressScreen
        snapshot={{
          ...snapshot,
          upgrade: {
            ...snapshot.upgrade!,
            current_step: 'verifying',
            steps: [
              {
                step: 'verifying',
                processed: 4098,
                total: 4098,
                unit: null,
                warning_count: 0,
                completed: false,
              },
            ],
          },
        }}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    expect(screen.queryByText(/100%/)).not.toBeInTheDocument()
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow')
  })
  it('shows step progress without calling representations history entries', () => {
    render(
      <StartupProgressScreen
        snapshot={makeUpgradePreview('upgrading', 20)}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50')
    expect(screen.queryByRole('button', { name: /重试|Retry/i })).not.toBeInTheDocument()
  })

  it('does not invent a percentage for unknown totals', () => {
    render(
      <StartupProgressScreen
        snapshot={makeUpgradePreview('unknown', 20)}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow')
  })

  it('only allows retry when the owner permits it', () => {
    const onRetry = vi.fn()
    render(
      <StartupProgressScreen
        snapshot={makeUpgradePreview('failed', 20)}
        onRetry={onRetry}
        onExport={vi.fn()}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /重试|Retry/i }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })

  it('does not offer password input for unavailable protection materials', () => {
    render(
      <StartupProgressScreen
        snapshot={makeUpgradePreview('protection', 20)}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /重试|Retry/i })).not.toBeInTheDocument()
  })
})
