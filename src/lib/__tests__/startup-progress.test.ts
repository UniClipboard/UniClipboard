import { describe, expect, it } from 'vitest'
import { makeUpgradePreview } from '@/dev/upgrade-preview-model'
import type { DaemonStartupStatus } from '@/lib/ipc'
import { startupViewSnapshot } from '@/lib/startup-progress'

describe('startup readiness presentation', () => {
  const ready: DaemonStartupStatus = {
    package_version: '1.0.0',
    service_ready: false,
    service_failed: false,
    progress: makeUpgradePreview('ready', 43),
  }
  it('does not present Engine readiness as a connected app', () => {
    expect(startupViewSnapshot(ready, false).state).toBe('starting_services')
    expect(startupViewSnapshot({ ...ready, service_ready: true }, false).state).toBe(
      'starting_services'
    )
  })
  it('makes a host service failure actionable without changing the Engine snapshot', () => {
    const result = startupViewSnapshot({ ...ready, service_failed: true }, false)
    expect(result.state).toBe('failed')
    expect(result.allowed_actions.retry).toBe(true)
    expect(ready.progress.state).toBe('ready')
  })
  it('hides the previous terminal state while a retry is in flight', () => {
    const failed = { ...ready, progress: makeUpgradePreview('failed', 20) }
    const result = startupViewSnapshot(failed, true)
    expect(result.state).toBe('preparing')
    expect(result.allowed_actions.retry).toBe(false)
  })
})
