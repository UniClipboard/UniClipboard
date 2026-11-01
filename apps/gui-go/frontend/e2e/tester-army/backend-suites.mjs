import { backendSuites } from './catalog.mjs'
export { backendSuites, verifiedBackendRuns, retainedBackendReasons } from './catalog.mjs'

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
