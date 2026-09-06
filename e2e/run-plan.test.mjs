import assert from 'node:assert/strict'
import { test } from 'vitest'
import { createSpecRuns } from './run-plan.mjs'

test('upgrade recovery notice runs every selected historical fixture', () => {
  const spec = '/workspace/e2e/specs/upgrade-re-pair-notice.e2e.js'
  const runs = createSpecRuns({
    specs: [spec],
    dualPeerMode: false,
    hostPlatform: 'darwin',
    hostArch: 'arm64',
  })

  assert.deepEqual(
    runs.map(run => run.env.E2E_UPGRADE_VERSION),
    ['0.19.1', '0.20.0-alpha.2', '0.20.0-alpha.6', '1.0.0-alpha.4']
  )
  assert.equal(new Set(runs.flatMap(run => run.profiles)).size, 4)
  assert.deepEqual(
    runs.map(run => run.fixture.directory),
    [
      'tests/e2e/fixtures/upgrades/v0.19.1/macos-aarch64/single-node-empty',
      'tests/e2e/fixtures/upgrades/v0.20.0-alpha.2/macos-aarch64/single-node-empty',
      'tests/e2e/fixtures/upgrades/v0.20.0-alpha.6/macos-aarch64/single-node-empty',
      'tests/e2e/fixtures/upgrades/v1.0.0-alpha.4/macos-aarch64/single-node-empty',
    ]
  )
  assert.equal(runs[0].env.E2E_UPGRADE_PASSPHRASE, 'v0-19-1-upgrade-fixture-passphrase')
  for (const run of runs.slice(1)) {
    assert.equal(run.env.E2E_UPGRADE_PASSPHRASE, 'upgrade-fixture-passphrase')
  }
})

test('upgrade recovery success runs every selected fixture with a fresh joiner', () => {
  const spec = '/workspace/e2e/specs/upgrade-re-pair-success.dual.e2e.js'
  const runs = createSpecRuns({
    specs: [spec],
    dualPeerMode: true,
    hostPlatform: 'darwin',
    hostArch: 'arm64',
  })

  assert.deepEqual(
    runs.map(run => run.env.E2E_UPGRADE_VERSION),
    ['0.19.1', '0.20.0-alpha.2', '0.20.0-alpha.6', '1.0.0-alpha.4']
  )
  assert.equal(new Set(runs.flatMap(run => run.profiles)).size, 8)
  for (const run of runs) {
    assert.equal(run.fixture.profile, run.env.E2E_UC_SPONSOR_PROFILE)
    assert.notEqual(run.env.E2E_UC_SPONSOR_PROFILE, run.env.E2E_UC_JOINER_PROFILE)
  }
})
