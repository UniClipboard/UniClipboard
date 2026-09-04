import path from 'node:path'

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
}) {
  return specs.map(spec => {
    const slug = scenarioSlug(spec)
    if (triplePeerMode) {
      const sponsor = `${sponsorProfile}-${slug}`
      const retained = `${retainedProfile}-${slug}`
      const removed = `${removedProfile}-${slug}`
      return {
        spec,
        profiles: [sponsor, retained, removed],
        env: {
          E2E_UC_SPONSOR_PROFILE: sponsor,
          E2E_UC_RETAINED_PROFILE: retained,
          E2E_UC_REMOVED_PROFILE: removed,
        },
      }
    }
    if (dualPeerMode) {
      const sponsor = `${sponsorProfile}-${slug}`
      const joiner = `${joinerProfile}-${slug}`
      return {
        spec,
        profiles: [sponsor, joiner],
        env: {
          E2E_UC_SPONSOR_PROFILE: sponsor,
          E2E_UC_JOINER_PROFILE: joiner,
        },
      }
    }

    const isolatedProfile = `${profile}-${slug}`
    return {
      spec,
      profiles: [isolatedProfile],
      env: { E2E_UC_PROFILE: isolatedProfile },
    }
  })
}
