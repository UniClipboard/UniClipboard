import path from 'node:path'

function scenarioSlug(spec) {
  return path
    .basename(spec)
    .replace(/\.dual\.e2e\.js$/, '')
    .replace(/\.e2e\.js$/, '')
    .replace(/[^a-zA-Z0-9-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .toLowerCase()
}

export function createSpecRuns({
  specs,
  dualPeerMode,
  profile = 'wdio',
  sponsorProfile = 'wdio-sponsor',
  joinerProfile = 'wdio-joiner',
}) {
  return specs.map(spec => {
    const slug = scenarioSlug(spec)
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
