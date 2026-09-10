import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { UpgradeProgressScreen } from '@/components/app/UpgradeProgressScreen'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'

describe('upgrade progress', () => {
  it('shows step progress without calling representations history entries', () => {
    render(
      <UpgradeProgressScreen
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
      <UpgradeProgressScreen
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
      <UpgradeProgressScreen
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
      <UpgradeProgressScreen
        snapshot={makeUpgradePreview('protection', 20)}
        onRetry={vi.fn()}
        onExport={vi.fn()}
      />
    )
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /重试|Retry/i })).not.toBeInTheDocument()
  })
})
