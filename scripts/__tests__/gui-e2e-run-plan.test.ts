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

  it('gives every triple-window spec three isolated profiles', () => {
    const runs = createSpecRuns({
      specs: ['/repo/e2e/specs/offline-removal-choice.triple.e2e.js'],
      dualPeerMode: false,
      triplePeerMode: true,
      sponsorProfile: 'wdio-sponsor',
      retainedProfile: 'wdio-retained',
      removedProfile: 'wdio-removed',
    })

    expect(runs).toEqual([
      {
        spec: '/repo/e2e/specs/offline-removal-choice.triple.e2e.js',
        profiles: [
          'wdio-sponsor-offline-removal-choice',
          'wdio-retained-offline-removal-choice',
          'wdio-removed-offline-removal-choice',
        ],
        env: {
          E2E_UC_SPONSOR_PROFILE: 'wdio-sponsor-offline-removal-choice',
          E2E_UC_RETAINED_PROFILE: 'wdio-retained-offline-removal-choice',
          E2E_UC_REMOVED_PROFILE: 'wdio-removed-offline-removal-choice',
        },
      },
    ])
  })

  it('attaches the tracked v0.19.1 userdata fixture to its macOS ARM upgrade spec', () => {
    const runs = createSpecRuns({
      specs: ['/repo/e2e/specs/upgrade-re-pair-notice.e2e.js'],
      dualPeerMode: false,
      triplePeerMode: false,
      profile: 'wdio',
      hostPlatform: 'darwin',
      hostArch: 'arm64',
    })

    expect(runs).toHaveLength(4)
    expect(runs[0]).toEqual({
      spec: '/repo/e2e/specs/upgrade-re-pair-notice.e2e.js',
      profiles: ['wdio-upgrade-re-pair-notice-0-19-1'],
      env: {
        E2E_UC_PROFILE: 'wdio-upgrade-re-pair-notice-0-19-1',
        E2E_UPGRADE_REPAIR: '1',
        E2E_UPGRADE_VERSION: '0.19.1',
        E2E_UPGRADE_PASSPHRASE: 'v0-19-1-upgrade-fixture-passphrase',
        E2E_SCREENSHOT_DIR: 'e2e/artifacts/upgrade-0-19-1',
      },
      fixture: {
        directory: 'tests/e2e/fixtures/upgrades/v0.19.1/macos-aarch64/single-node-empty',
        profile: 'wdio-upgrade-re-pair-notice-0-19-1',
      },
    })
  })
})
