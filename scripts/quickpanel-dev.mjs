// Development launcher for the GPUI quick panel.
//
//   bun quickpanel:dev                      profile `dev`
//   bun quickpanel:dev:profile <profile>    any other development profile
//   bun quickpanel:dev --profile <profile>  the same, with the flag spelled out
//   bun quickpanel:dev --no-daemon          only run the panel (a daemon of that profile is already
//                                           running, for example from `bun wails:dev`)
//
// It builds and starts `uniclipd` for the profile, then runs the panel against it with
// `cargo run -p quick-panel`. Stopping this command stops both. Starting a second daemon for a
// profile that already has one makes the two compete for the profile (single-instance takeover),
// so use `--no-daemon` whenever the profile is already served.
// Setup and unlock of the profile still happen in the GUI; the panel only reads the history.
import { spawn } from 'node:child_process'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { PROFILE_PATTERN } from './dev-profile.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

export const USAGE = `Usage: bun quickpanel:dev [--profile <profile>] [--no-daemon]
       bun quickpanel:dev:profile <profile> [--no-daemon]

Builds and starts uniclipd and the GPUI quick panel for a development profile
(default: dev). Both stop when this command stops.`

export function parseArgs(argv, env = process.env) {
  const options = { profile: env.UC_PROFILE || 'dev', daemon: true, help: false }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--help' || arg === '-h') options.help = true
    else if (arg === '--no-daemon') options.daemon = false
    else if (arg === '--profile') options.profile = argv[++i]
    else throw new Error(`Unknown argument: ${arg}\n\n${USAGE}`)
  }
  if (!options.help && (!options.profile || !PROFILE_PATTERN.test(options.profile))) {
    throw new Error(
      `Invalid profile name. Use 1-64 letters, numbers, underscores, or hyphens, starting with a letter or number.\n\n${USAGE}`
    )
  }
  return options
}

function build(args) {
  return new Promise(resolve => {
    spawn('cargo', args, { cwd: root, stdio: 'inherit' }).once('exit', code => resolve(code ?? 1))
  })
}

export async function main(argv, env = process.env) {
  let options
  try {
    options = parseArgs(argv, env)
  } catch (error) {
    console.error(error.message)
    return 1
  }
  if (options.help) {
    console.log(USAGE)
    return 0
  }
  const { profile } = options
  const packages = options.daemon ? ['-p', 'uc-daemon', '-p', 'quick-panel'] : ['-p', 'quick-panel']
  console.log(
    `[quickpanel-dev] profile=${profile} daemon=${options.daemon ? 'started here' : 'external'}`
  )
  if ((await build(['build', ...packages])) !== 0) return 1

  const childEnv = { ...env, UNICLIPBOARD_ENV: 'development', UC_PROFILE: profile }
  const exe = name =>
    path.join(root, 'target/debug', process.platform === 'win32' ? `${name}.exe` : name)
  const children = []
  let stopping = false
  const stop = () => {
    stopping = true
    for (const child of children)
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM')
  }
  process.on('SIGINT', stop)
  process.on('SIGTERM', stop)
  try {
    if (options.daemon) {
      const daemon = spawn(exe('uniclipd'), [], { cwd: root, env: childEnv, stdio: 'inherit' })
      children.push(daemon)
      daemon.once('error', error => {
        console.error(`[quickpanel-dev] failed to start uniclipd: ${error.message}`)
        stop()
      })
      daemon.once('exit', () => {
        if (!stopping) console.error('[quickpanel-dev] uniclipd exited')
        stop()
      })
    }
    const panel = spawn(exe('uniclip-quick-panel'), [], {
      cwd: root,
      env: childEnv,
      stdio: 'inherit',
    })
    children.push(panel)
    const code = await new Promise(resolve => {
      panel.once('exit', (status, signal) => resolve(status ?? (signal ? 1 : 0)))
      panel.once('error', error => {
        console.error(`[quickpanel-dev] failed to start the panel: ${error.message}`)
        resolve(1)
      })
    })
    return stopping && code === 1 ? 130 : code
  } finally {
    stop()
    process.off('SIGINT', stop)
    process.off('SIGTERM', stop)
  }
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (isMain) {
  process.exitCode = await main(process.argv.slice(2))
}
