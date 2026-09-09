import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { createServer } from 'node:net'
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
An occupied frontend port is replaced with an available port for this launch.

Examples:
  bun tauri:dev:profile alice
  bun tauri:dev:profile alice -- --no-dev-server-wait`

export function createTauriDevInvocation(args, currentEnv = process.env, portOverride) {
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
  const devServerPort = portOverride ?? devServerPortForProfile(profile)
  const profileConfig = JSON.stringify({
    build: { devUrl: `http://localhost:${devServerPort}` },
  })
  const separatorIndex = tauriArgs.indexOf('--')
  const configuredTauriArgs =
    separatorIndex === -1
      ? [...tauriArgs, '--config', profileConfig]
      : [
          ...tauriArgs.slice(0, separatorIndex),
          '--config',
          profileConfig,
          ...tauriArgs.slice(separatorIndex),
        ]

  return {
    command: 'bun',
    args: ['run', 'tauri', '--', 'dev', ...configuredTauriArgs],
    env: {
      ...currentEnv,
      UNICLIPBOARD_ENV: 'development',
      UC_PROFILE: profile,
      UC_DEV_SERVER_PORT: String(devServerPort),
    },
  }
}

function availablePort(port, host) {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.once('error', reject)
    server.listen({ port, host, exclusive: true }, () => {
      const selectedPort = server.address().port
      server.close(error => (error ? reject(error) : resolve(selectedPort)))
    })
  })
}

async function availableDevPort(port, host) {
  if (host) return availablePort(port, host)
  // macOS permits a wildcard listener alongside an existing loopback listener.
  // Probe each exact loopback address so another worktree cannot satisfy Tauri's wait.
  const selectedPort = await availablePort(port, '127.0.0.1')
  try {
    await availablePort(selectedPort, '::1')
  } catch (error) {
    if (error.code !== 'EAFNOSUPPORT' && error.code !== 'EADDRNOTAVAIL') throw error
  }
  return selectedPort
}

export async function createAvailableTauriDevInvocation(args, currentEnv = process.env) {
  const invocation = createTauriDevInvocation(args, currentEnv)
  const preferredPort = Number(invocation.env.UC_DEV_SERVER_PORT)
  let port
  try {
    port = await availableDevPort(preferredPort, currentEnv.TAURI_DEV_HOST)
  } catch (error) {
    if (error.code !== 'EADDRINUSE') throw error
    port = await availableDevPort(0, currentEnv.TAURI_DEV_HOST)
  }
  return createTauriDevInvocation(args, currentEnv, port)
}

export async function runTauriDevProfile(args, currentEnv = process.env) {
  if (args.length === 1 && (args[0] === '--help' || args[0] === '-h')) {
    console.log(USAGE)
    return 0
  }

  let invocation
  try {
    invocation = await createAvailableTauriDevInvocation(args, currentEnv)
  } catch (error) {
    console.error(`${error instanceof Error ? error.message : String(error)}\n\n${USAGE}`)
    return 1
  }

  const preferredPort = devServerPortForProfile(args[0])
  if (Number(invocation.env.UC_DEV_SERVER_PORT) !== preferredPort) {
    console.log(
      `[tauri-dev-profile] port ${preferredPort} is occupied; using ${invocation.env.UC_DEV_SERVER_PORT}`
    )
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
  process.exitCode = await runTauriDevProfile(process.argv.slice(2))
}
