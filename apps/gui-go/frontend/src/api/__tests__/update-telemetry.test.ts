/**
 * Unit tests for update-telemetry routing (ADR-008 D20).
 *
 * The capture wrappers must route through the daemon `POST /analytics/capture`
 * wrapper (`@/api/daemon/analytics`), NOT the retired in-process Tauri command.
 * `dialog_opened` must forward an `install_kind` probed natively, and fall back
 * to `unknown` when the probe rejects.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const captureUiEvent = vi.fn().mockResolvedValue(undefined)
const getInstallKind = vi.fn().mockResolvedValue('deb')

vi.mock('@/api/daemon/analytics', () => ({
  captureUiEvent: (...args: unknown[]) => captureUiEvent(...args),
}))

vi.mock('../updater', () => ({
  getInstallKind: (...args: unknown[]) => getInstallKind(...args),
}))

const { captureUpdateDialogOpened, captureUpdateDismissed, captureUpdateActionInvoked } =
  await import('../update-telemetry')

/** Let the fire-and-forget promise chain settle. */
async function flush(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
}

beforeEach(() => {
  captureUiEvent.mockClear()
  getInstallKind.mockClear()
  getInstallKind.mockResolvedValue('deb')
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('captureUpdateDialogOpened', () => {
  it('forwards the natively-probed install_kind', async () => {
    captureUpdateDialogOpened('notification', 'available')
    await flush()

    expect(getInstallKind).toHaveBeenCalledTimes(1)
    expect(captureUiEvent).toHaveBeenCalledWith({
      kind: 'dialog_opened',
      source: 'notification',
      phase: 'available',
      install_kind: 'deb',
    })
  })

  it('falls back to "unknown" when the install-kind probe rejects', async () => {
    getInstallKind.mockRejectedValueOnce(new Error('probe failed'))
    captureUpdateDialogOpened('sidebar_icon', 'ready')
    await flush()

    expect(captureUiEvent).toHaveBeenCalledWith({
      kind: 'dialog_opened',
      source: 'sidebar_icon',
      phase: 'ready',
      install_kind: 'unknown',
    })
  })
})

describe('captureUpdateDismissed', () => {
  it('routes through the daemon wrapper without an install_kind probe', async () => {
    captureUpdateDismissed('available', 'dialog_later')
    await flush()

    expect(getInstallKind).not.toHaveBeenCalled()
    expect(captureUiEvent).toHaveBeenCalledWith({
      kind: 'dismissed',
      phase: 'available',
      source: 'dialog_later',
    })
  })
})

describe('captureUpdateActionInvoked', () => {
  it('maps an omitted errorKind to null', async () => {
    captureUpdateActionInvoked('download_bg', 'cancelled')
    await flush()

    expect(captureUiEvent).toHaveBeenCalledWith({
      kind: 'action_invoked',
      action: 'download_bg',
      outcome: 'cancelled',
      error_kind: null,
    })
  })

  it('preserves a provided errorKind', async () => {
    captureUpdateActionInvoked('install', 'failed', 'signature_mismatch')
    await flush()

    expect(captureUiEvent).toHaveBeenCalledWith({
      kind: 'action_invoked',
      action: 'install',
      outcome: 'failed',
      error_kind: 'signature_mismatch',
    })
  })
})
