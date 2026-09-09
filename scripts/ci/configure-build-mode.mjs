import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

export function resolveBuildMode(mode) {
  if (mode === 'release') return { cargoEnv: {}, suffix: '' }
  if (mode !== 'test') throw new Error(`Unknown build mode: ${mode}`)
  return {
    cargoEnv: {
      CARGO_PROFILE_RELEASE_OPT_LEVEL: '1',
      CARGO_PROFILE_RELEASE_LTO: 'off',
      CARGO_PROFILE_RELEASE_CODEGEN_UNITS: '16',
    },
    suffix: '-test',
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const { cargoEnv, suffix } = resolveBuildMode(process.env.UC_BUILD_MODE ?? 'release')
  if (!process.env.GITHUB_ENV || !process.env.GITHUB_OUTPUT) {
    throw new Error('GITHUB_ENV and GITHUB_OUTPUT are required')
  }
  // Use release-mode packaging without changing debug assertions, panic handling, or symbols.
  for (const [key, value] of Object.entries(cargoEnv)) {
    appendFileSync(process.env.GITHUB_ENV, `${key}=${value}\n`)
  }
  appendFileSync(process.env.GITHUB_OUTPUT, `suffix=${suffix}\n`)
}
