import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

// Each reusable workflow invocation owns one sidecar-to-package dependency chain.
export function selectBuildPlatforms(platform, { buildMode = 'release', buildOnly = false } = {}) {
  const desktop = ['macos-aarch64', 'macos-x86_64', 'windows-x86_64', 'windows-arm64']
  if (platform === 'all') {
    return buildMode === 'release' && !buildOnly ? [...desktop, 'linux-gui'] : desktop
  }
  if (platform === 'windows-latest') return ['windows-x86_64', 'windows-arm64']
  if ([...desktop, 'linux-gui', 'ubuntu-22.04', 'ubuntu-22.04-arm'].includes(platform)) {
    return [platform]
  }
  throw new Error(`Unknown build platform: ${platform}`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!process.env.GITHUB_OUTPUT) throw new Error('GITHUB_OUTPUT is required')
  const platforms = selectBuildPlatforms(process.env.UC_PLATFORM || 'all', {
    buildMode: process.env.UC_BUILD_MODE || 'release',
    buildOnly: process.env.UC_BUILD_ONLY === 'true',
  })
  appendFileSync(process.env.GITHUB_OUTPUT, `platforms=${JSON.stringify(platforms)}\n`)
}
