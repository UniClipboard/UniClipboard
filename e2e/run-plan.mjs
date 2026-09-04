import path from 'node:path'

const upgradeFixtures = [
  {
    version: '0.19.1',
    passphrase: 'v0-19-1-upgrade-fixture-passphrase',
  },
  {
    version: '0.20.0-alpha.2',
    passphrase: 'upgrade-fixture-passphrase',
  },
  {
    version: '0.20.0-alpha.6',
    passphrase: 'upgrade-fixture-passphrase',
  },
  {
    version: '1.0.0-alpha.4',
    passphrase: 'upgrade-fixture-passphrase',
  },
]

function scenarioSlug(spec) {
  return path
    .basename(spec)
    .replace(/\.triple\.e2e\.js$/, '')
    .replace(/\.dual\.e2e\.js$/, '')
    .replace(/\.e2e\.js$/, '')
    .replace(/[^a-zA-Z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .toLowerCase()
}

export function createSpecRuns({
  specs,
  dualPeerMode,
  triplePeerMode = false,
  profile = 'wdio',
  sponsorProfile = 'wdio-sponsor',
  joinerProfile = 'wdio-joiner',
  retainedProfile = 'wdio-retained',
  removedProfile = 'wdio-removed',
  hostPlatform,
  hostArch,
}) {
  return specs.flatMap(spec => {
    const slug = scenarioSlug(spec)
    if (triplePeerMode) {
      const sponsor = `${sponsorProfile}-${slug}`
      const retained = `${retainedProfile}-${slug}`
      const removed = `${removedProfile}-${slug}`
      return [
        {
          spec,
          profiles: [sponsor, retained, removed],
          env: {
            E2E_UC_SPONSOR_PROFILE: sponsor,
            E2E_UC_RETAINED_PROFILE: retained,
            E2E_UC_REMOVED_PROFILE: removed,
          },
        },
      ]
    }
    if (dualPeerMode) {
      if (slug === 'upgrade-re-pair-success' && hostPlatform === 'darwin' && hostArch === 'arm64') {
        return upgradeFixtures.map(fixture => {
          const versionSlug = fixture.version.replaceAll('.', '-').replaceAll('+', '-')
          const sponsor = `${sponsorProfile}-${slug}-${versionSlug}`
          const joiner = `${joinerProfile}-${slug}-${versionSlug}`
          return {
            spec,
            profiles: [sponsor, joiner],
            env: {
              E2E_UC_SPONSOR_PROFILE: sponsor,
              E2E_UC_JOINER_PROFILE: joiner,
              E2E_UPGRADE_REPAIR: '1',
              E2E_UPGRADE_VERSION: fixture.version,
              E2E_UPGRADE_PASSPHRASE: fixture.passphrase,
              E2E_SCREENSHOT_DIR: path.join('e2e', 'artifacts', `upgrade-success-${versionSlug}`),
            },
            fixture: {
              directory: `tests/e2e/fixtures/upgrades/v${fixture.version}/macos-aarch64/single-node-empty`,
              profile: sponsor,
            },
          }
        })
      }
      const sponsor = `${sponsorProfile}-${slug}`
      const joiner = `${joinerProfile}-${slug}`
      return [
        {
          spec,
          profiles: [sponsor, joiner],
          env: {
            E2E_UC_SPONSOR_PROFILE: sponsor,
            E2E_UC_JOINER_PROFILE: joiner,
          },
        },
      ]
    }

    const isolatedProfile = `${profile}-${slug}`
    if (slug === 'upgrade-re-pair-notice' && hostPlatform === 'darwin' && hostArch === 'arm64') {
      return upgradeFixtures.map(fixture => {
        const versionSlug = fixture.version.replaceAll('.', '-').replaceAll('+', '-')
        const upgradeProfile = `${isolatedProfile}-${versionSlug}`
        return {
          spec,
          profiles: [upgradeProfile],
          env: {
            E2E_UC_PROFILE: upgradeProfile,
            E2E_UPGRADE_REPAIR: '1',
            E2E_UPGRADE_VERSION: fixture.version,
            E2E_UPGRADE_PASSPHRASE: fixture.passphrase,
            E2E_SCREENSHOT_DIR: path.join('e2e', 'artifacts', `upgrade-${versionSlug}`),
          },
          fixture: {
            directory: `tests/e2e/fixtures/upgrades/v${fixture.version}/macos-aarch64/single-node-empty`,
            profile: upgradeProfile,
          },
        }
      })
    }
    return [
      {
        spec,
        profiles: [isolatedProfile],
        env: { E2E_UC_PROFILE: isolatedProfile },
      },
    ]
  })
}
