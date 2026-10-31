export const backendSuites = {
  'tag-manager-layout': { platforms: ['macos', 'linux'], seeds: ['empty'] },
  'history-kit': { platforms: ['macos', 'windows', 'linux'], seeds: ['history', 'empty'] },
  'devices-kit': { platforms: ['macos', 'windows', 'linux'], seeds: ['devices'] },
  'quick-panel-unlock': { platforms: ['linux', 'macos', 'windows'], seeds: ['panel'] },
  'sidebar-toggle-app': { platforms: ['macos'], seeds: ['empty'] },
  'history-search': { platforms: ['none'], seeds: ['search'] },
}
export const backendName = process.env.UC_TESTER_BACKEND_SUITE || 'tag-manager-layout'
export const backendPlatform = process.env.UC_TESTER_PLATFORM || 'macos'
export const backendSeed =
  process.env.UC_TESTER_BACKEND_SEED || backendSuites[backendName]?.seeds[0]
if (
  !backendSuites[backendName]?.platforms.includes(backendPlatform) ||
  !backendSuites[backendName]?.seeds.includes(backendSeed)
)
  throw new Error('Unsupported backend suite/platform/seed')
export const backendPhase = process.env.UC_TESTER_BACKEND_PHASE || 'all'
if (
  backendPhase !== 'all' &&
  (backendName !== 'history-search' ||
    !['components', 'fullPage', 'fullPageWindows', 'fullPageLinuxSystemFrame'].includes(
      backendPhase
    ))
)
  throw new Error('Unsupported backend phase')
export const backendOutput = `target/tester-army-backend/${backendPlatform}/${backendName}/${backendSeed}/${backendPhase}`
export const backendReport = `target/tester-army-backend-report/${backendPlatform}/${backendName}/${backendSeed}/${backendPhase}`

// Each entry has passed its stated functional contract; retained candidates stay explicit.
export const verifiedBackendRuns = [
  { suite: 'sidebar-toggle-app', platform: 'macos', seed: 'empty' },
  ...['macos', 'linux'].map(platform => ({ suite: 'tag-manager-layout', platform, seed: 'empty' })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'quick-panel-unlock',
    platform,
    seed: 'panel',
  })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'devices-kit',
    platform,
    seed: 'devices',
  })),
  ...['macos', 'windows', 'linux'].map(platform => ({
    suite: 'history-kit',
    platform,
    seed: 'empty',
  })),
  { suite: 'history-search', platform: 'none', seed: 'search', phase: 'components' },
]
export const retainedBackendReasons = {
  'history-kit/history':
    'LAN image thumbnail returns 503 while original PNG bytes are readable; platform-specific material/rail assumptions also remain in the original scenarios',
  'history-search/fullPage':
    'original WebDriver and replacement both fail the old detail geometry predicate; legacy rail assumptions remain in the Windows/Linux branches',
}
