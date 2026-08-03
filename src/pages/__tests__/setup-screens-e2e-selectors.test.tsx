import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import i18n from '@/i18n'
import {
  EntryScreen,
  InitializeSpaceScreen,
  PairingCompleteScreen,
  RedeemInvitationScreen,
  ShowInvitationScreen,
  SpaceReadyScreen,
} from '@/pages/setup/screens'

describe('setup screens e2e selectors', () => {
  beforeAll(() => {
    if (!('ResizeObserver' in globalThis)) {
      Object.defineProperty(globalThis, 'ResizeObserver', {
        configurable: true,
        value: class ResizeObserver {
          observe() {}
          unobserve() {}
          disconnect() {}
        },
      })
    }

    if (!document.elementFromPoint) {
      Object.defineProperty(document, 'elementFromPoint', {
        configurable: true,
        value: vi.fn(() => document.body),
      })
    }
  })

  // input-otp@1.4.2 schedules three unguarded setTimeouts (0/10/50ms) from a
  // useEffect with no cleanup. After unmount they still fire and call
  // dispatchSetState; if jsdom is torn down first the deferred update throws
  // `window is not defined` as an unhandled error and fails the run.
  afterEach(async () => {
    cleanup()
    await new Promise(resolve => setTimeout(resolve, 60))
  })

  it('lets a sponsor continue directly into device invitation from the space-ready screen', async () => {
    const user = userEvent.setup()
    const onInvite = vi.fn().mockResolvedValue({ ok: true })
    const onDone = vi.fn()

    render(<SpaceReadyScreen onInvite={onInvite} onDone={onDone} />)

    await user.click(screen.getByTestId('setup-complete-invite'))
    expect(onInvite).toHaveBeenCalledTimes(1)
    expect(onDone).not.toHaveBeenCalled()

    await user.click(screen.getByTestId('setup-complete-later'))
    expect(onDone).toHaveBeenCalledTimes(1)
  })

  it('keeps sponsor pairing success focused on entering the app', async () => {
    const user = userEvent.setup()
    const onInvite = vi.fn().mockResolvedValue({ ok: true })
    const onDone = vi.fn()

    render(<PairingCompleteScreen onDone={onDone} />)

    expect(screen.queryByTestId('setup-complete-invite')).not.toBeInTheDocument()
    await user.click(screen.getByTestId('setup-complete-done'))
    expect(onDone).toHaveBeenCalledTimes(1)
    expect(onInvite).not.toHaveBeenCalled()
  })

  it('returns from an invitation as soon as its code expires', async () => {
    const onCancel = vi.fn()

    render(<ShowInvitationScreen code="ABC123" expiresAtMs={Date.now() - 1} onCancel={onCancel} />)

    await waitFor(() => expect(onCancel).toHaveBeenCalledTimes(1))
  })

  it('keeps the joiner completion focused on entering the app', async () => {
    const user = userEvent.setup()
    const onInvite = vi.fn().mockResolvedValue({ ok: true })
    const onDone = vi.fn()

    render(<PairingCompleteScreen onDone={onDone} />)

    expect(screen.queryByTestId('setup-complete-invite')).not.toBeInTheDocument()
    await user.click(screen.getByTestId('setup-complete-done'))
    expect(onDone).toHaveBeenCalledTimes(1)
    expect(onInvite).not.toHaveBeenCalled()
  })

  it('clears the consumed invitation after a wrong passphrase', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn().mockResolvedValue({
      ok: false,
      kind: 'passphrase_mismatch',
      raw: 'wrong passphrase',
    })

    render(<RedeemInvitationScreen onSubmit={onSubmit} onBack={vi.fn()} />)

    const codeInput = screen.getByLabelText('Invitation code')
    await user.type(codeInput, 'ABCD1234')
    const passphraseInput = await screen.findByLabelText('Space passphrase')
    await user.type(passphraseInput, 'wrong passphrase')
    await user.click(screen.getByTestId('setup-redeem-submit'))

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    expect(codeInput).toHaveValue('')
    expect(screen.queryByLabelText('Space passphrase')).not.toBeInTheDocument()

    await user.type(codeInput, 'WXYZ5678')
    expect(await screen.findByLabelText('Space passphrase')).toHaveValue('')
  })

  it('keeps the invitation and passphrase when the other device needs an update', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn().mockResolvedValue({
      ok: false,
      kind: 'sponsor_upgrade_required',
      raw: 'sponsor must upgrade before pairing',
    })

    render(<RedeemInvitationScreen onSubmit={onSubmit} onBack={vi.fn()} />)

    const codeInput = screen.getByLabelText('Invitation code')
    await user.type(codeInput, 'ABCD1234')
    const passphraseInput = await screen.findByLabelText('Space passphrase')
    await user.type(passphraseInput, 'correct passphrase')
    await user.click(screen.getByTestId('setup-redeem-submit'))

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    expect(codeInput).toHaveValue('ABCD1234')
    expect(passphraseInput).toHaveValue('correct passphrase')
    expect(
      screen.getByText(i18n.t('setup.redeemInvitation.errors.sponsorUpgradeRequired'))
    ).toBeInTheDocument()
  })

  it('exposes stable controls for the real-window setup smoke test', () => {
    const noop = vi.fn()

    const { rerender } = render(<EntryScreen onCreate={noop} onJoin={noop} onImport={noop} />)
    expect(screen.getByTestId('setup-entry-create')).toBeInTheDocument()
    expect(screen.getByTestId('setup-entry-join')).toBeInTheDocument()
    expect(screen.getByTestId('setup-entry-import')).toBeInTheDocument()

    rerender(<InitializeSpaceScreen onSubmit={vi.fn()} onBack={noop} />)
    expect(screen.getByTestId('setup-initialize-back')).toBeInTheDocument()
    expect(screen.getByTestId('setup-initialize-submit')).toBeInTheDocument()

    rerender(<RedeemInvitationScreen onSubmit={vi.fn()} onBack={noop} />)
    expect(screen.getByTestId('setup-redeem-back')).toBeInTheDocument()
    expect(screen.getByTestId('setup-redeem-code')).toBeInTheDocument()
    expect(screen.getByTestId('setup-redeem-submit')).toBeInTheDocument()

    rerender(
      <ShowInvitationScreen code="ABCD1234" expiresAtMs={Date.now() + 60_000} onCancel={noop} />
    )
    expect(screen.getByTestId('setup-invitation-code')).toHaveTextContent('ABCD-1234')
    expect(screen.getByTestId('setup-invitation-cancel')).toBeInTheDocument()

    rerender(<PairingCompleteScreen peerDeviceId="peer-device-id" onDone={noop} />)
    expect(screen.getByTestId('setup-pairing-complete')).toBeInTheDocument()
    expect(screen.getByTestId('setup-complete-peer-id')).toHaveTextContent('peer-device-id')
  })
})
