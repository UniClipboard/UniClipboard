import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const PROFILE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/
const PROFILE_PORT_BASE = 20_000
const PROFILE_PORT_RANGE = 20_000

export function devServerPortForProfile(profile) {
  const digest = createHash('sha256').update(profile).digest()
  return PROFILE_PORT_BASE + (digest.readUInt32BE(0) % PROFILE_PORT_RANGE)
}

export const USAGE = `Usage: bun tauri:dev:profile <profile> [-- <tauri arguments...>]

Profile names may contain 1-64 letters, numbers, underscores, or hyphens,
and must start with a letter or number.

Examples:
  bun tauri:dev:profile alice
  bun tauri:dev:profile alice -- --no-dev-server-wait`

export function createTauriDevInvocation(args, currentEnv = process.env) {
  const [profile, ...remainingArgs] = args
  if (profile === undefined) {
    throw new Error('Profile name is required.')
  }
  if (!PROFILE_PATTERN.test(profile)) {
    throw new Error(
      'Invalid profile name. Use 1-64 letters, numbers, underscores, or hyphens, starting with a letter or number.'
    )
  }

  const tauriArgs = remainingArgs[0] === '--' ? remainingArgs.slice(1) : remainingArgs
  const devServerPort = devServerPortForProfile(profile)
  const profileConfig = JSON.stringify({
    build: { devUrl: `http://localhost:${devServerPort}` },
  })

  return {
    command: 'bun',
    args: ['run', 'tauri', '--', 'dev', ...tauriArgs, '--config', profileConfig],
    env: {
      ...currentEnv,
      UNICLIPBOARD_ENV: 'development',
      UC_PROFILE: profile,
      UC_DEV_SERVER_PORT: String(devServerPort),
    },
  }
}

export function runTauriDevProfile(args, currentEnv = process.env) {
  if (args.length === 1 && (args[0] === '--help' || args[0] === '-h')) {
    console.log(USAGE)
    return 0
  }

  let invocation
  try {
    invocation = createTauriDevInvocation(args, currentEnv)
  } catch (error) {
    console.error(`${error instanceof Error ? error.message : String(error)}\n\n${USAGE}`)
    return 1
  }

  const result = spawnSync(invocation.command, invocation.args, {
    env: invocation.env,
    stdio: 'inherit',
  })
  if (result.error) {
    console.error(`Failed to start Tauri development mode: ${result.error.message}`)
    return 1
  }

  return result.status ?? 1
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  process.exitCode = runTauriDevProfile(process.argv.slice(2))
}
