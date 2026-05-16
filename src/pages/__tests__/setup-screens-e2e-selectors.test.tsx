import { render, screen } from '@testing-library/react'
import { beforeAll, describe, expect, it, vi } from 'vitest'
import { EntryScreen, InitializeSpaceScreen, RedeemInvitationScreen } from '@/pages/setup/screens'

describe('setup screens e2e selectors', () => {
  beforeAll(() => {
    if ('ResizeObserver' in globalThis) return

    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class ResizeObserver {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    })
  })

  it('exposes stable controls for the real-window setup smoke test', () => {
    const noop = vi.fn()

    const { rerender } = render(<EntryScreen onCreate={noop} onJoin={noop} />)
    expect(screen.getByTestId('setup-entry-create')).toBeInTheDocument()
    expect(screen.getByTestId('setup-entry-join')).toBeInTheDocument()

    rerender(<InitializeSpaceScreen onSubmit={vi.fn()} onBack={noop} />)
    expect(screen.getByTestId('setup-initialize-back')).toBeInTheDocument()
    expect(screen.getByTestId('setup-initialize-submit')).toBeInTheDocument()

    rerender(<RedeemInvitationScreen onSubmit={vi.fn()} onBack={noop} />)
    expect(screen.getByTestId('setup-redeem-back')).toBeInTheDocument()
    expect(screen.getByTestId('setup-redeem-code')).toBeInTheDocument()
    expect(screen.getByTestId('setup-redeem-submit')).toBeInTheDocument()
  })
})
