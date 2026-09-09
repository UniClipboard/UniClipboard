import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export function resolveBuildMode(mode, cacheWorkspace = false) {
  if (mode !== 'release' && mode !== 'test') throw new Error(`Unknown build mode: ${mode}`)
  if (cacheWorkspace && mode !== 'test')
    throw new Error('Workspace cache experiment requires test mode')
  const suffix = mode === 'test' ? '-test' : ''
  return {
    cargoEnv:
      mode === 'test'
        ? {
            CARGO_PROFILE_RELEASE_OPT_LEVEL: '1',
            CARGO_PROFILE_RELEASE_LTO: 'off',
            CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16',
          }
        : {},
    suffix,
    cacheSuffix: `${suffix}${cacheWorkspace ? '-workspace' : ''}`,
    cacheWorkspace,
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const workspaceInput = process.env.UC_CACHE_WORKSPACE ?? 'false'
  if (!['false', 'true'].includes(workspaceInput))
    throw new Error('UC_CACHE_WORKSPACE must be true or false')
  const { cargoEnv, suffix, cacheSuffix, cacheWorkspace } = resolveBuildMode(
    process.env.UC_BUILD_MODE ?? 'release',
    workspaceInput === 'true'
  )
  if (!process.env.GITHUB_ENV || !process.env.GITHUB_OUTPUT) {
    throw new Error('GITHUB_ENV and GITHUB_OUTPUT are required')
  }
  // Use release-mode packaging without changing debug assertions, panic handling, or symbols.
  for (const [key, value] of Object.entries(cargoEnv)) {
    appendFileSync(process.env.GITHUB_ENV, `${key}=${value}\n`)
  }
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `suffix=${suffix}\ncache_suffix=${cacheSuffix}\ncache_workspace=${cacheWorkspace}\n`
  )
}
