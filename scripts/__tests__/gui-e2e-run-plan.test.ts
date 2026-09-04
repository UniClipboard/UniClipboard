import { describe, expect, it } from 'vitest'
import { createSpecRuns } from '../../e2e/run-plan.mjs'

describe('GUI E2E run plan', () => {
  it('gives every single-window spec its own profile', () => {
    const runs = createSpecRuns({
      specs: ['/repo/e2e/specs/setup-smoke.e2e.js', '/repo/e2e/specs/quick-panel.e2e.js'],
      dualPeerMode: false,
      profile: 'wdio',
    })

    expect(runs).toEqual([
      {
        spec: '/repo/e2e/specs/setup-smoke.e2e.js',
        profiles: ['wdio-setup-smoke'],
        env: { E2E_UC_PROFILE: 'wdio-setup-smoke' },
      },
      {
        spec: '/repo/e2e/specs/quick-panel.e2e.js',
        profiles: ['wdio-quick-panel'],
        env: { E2E_UC_PROFILE: 'wdio-quick-panel' },
      },
    ])
  })

  it('gives every dual-window spec its own sponsor and joiner profiles', () => {
    const runs = createSpecRuns({
      specs: [
        '/repo/e2e/specs/setup-pairing-success.dual.e2e.js',
        '/repo/e2e/specs/file-transfer-cancel.dual.e2e.js',
      ],
      dualPeerMode: true,
      sponsorProfile: 'wdio-sponsor',
      joinerProfile: 'wdio-joiner',
    })

    expect(runs.map(run => run.profiles)).toEqual([
      ['wdio-sponsor-setup-pairing-success', 'wdio-joiner-setup-pairing-success'],
      ['wdio-sponsor-file-transfer-cancel', 'wdio-joiner-file-transfer-cancel'],
    ])
    expect(new Set(runs.flatMap(run => run.profiles)).size).toBe(4)
  })
})
