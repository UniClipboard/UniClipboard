import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
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

  // input-otp@1.4.2 schedules three unguarded setTimeouts (0/10/50ms) from a
  // useEffect with no cleanup. After unmount they still fire and call
  // dispatchSetState; if jsdom is torn down first the deferred update throws
  // `window is not defined` as an unhandled error and fails the run.
  afterEach(async () => {
    cleanup()
    await new Promise(resolve => setTimeout(resolve, 60))
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
