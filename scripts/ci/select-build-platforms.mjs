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
