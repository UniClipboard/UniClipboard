import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import { suites, verifiedPlatforms, retainedReasons } from './suites.mjs'

const args = process.argv.slice(2)
const named = []
let platform
for (let index = 0; index < args.length; index++) {
  const arg = args[index]
  if (arg === '--platform') platform = args[++index]
  else if (arg.startsWith('--platform=')) platform = arg.split('=')[1]
  else if (arg.startsWith('--')) throw new Error(`Unknown option: ${arg}`)
  else named.push(arg)
}
const selected = named.length ? named : Object.keys(verifiedPlatforms)
if (!named.length) {
  for (const [name, reason] of Object.entries(retainedReasons))
    console.log(`RETAINED ${name}: ${reason}`)
}
let failed = false
let active
let interrupted
for (const signal of ['SIGINT', 'SIGTERM'])
  process.once(signal, () => {
    interrupted = signal
    active?.kill(signal)
  })
const runs = []
await mkdir('target/tester-army-layout-report', { recursive: true })
suiteLoop: for (const name of selected) {
  if (interrupted) break
  if (!suites[name]) throw new Error(`Unknown browser suite: ${name}`)
  if (platform && !suites[name].platforms.includes(platform)) {
    if (named.length) throw new Error(`Unsupported platform ${platform} for ${name}`)
    continue
  }
  for (const skin of (named.length ? suites[name].platforms : verifiedPlatforms[name]).filter(
    skin => !platform || skin === platform
  )) {
    const proc = spawn(
      process.execPath,
      ['node_modules/e2e/dist/cli/bin.js', 'run', '--config', 'e2e.layouts.config.ts'],
      {
        env: {
          ...process.env,
          E2E_TELEMETRY_DISABLED: '1',
          UC_TESTER_SUITE: name,
          UC_TESTER_PLATFORM: skin,
        },
        stdio: 'inherit',
      }
    )
    active = proc
    const code = await new Promise((resolve, reject) => {
      proc.once('error', reject)
      proc.once('exit', resolve)
    })
    active = undefined
    runs.push({ suite: name, platform: skin, exitCode: code })
    await writeFile(
      'target/tester-army-layout-report/selection.json',
      JSON.stringify(
        {
          scope: named.length ? 'explicit-candidate' : 'verified-combinations',
          runs,
          retained: retainedReasons,
        },
        null,
        2
      ) + '\n'
    )
    if (code !== 0) failed = true
    if (interrupted) break suiteLoop
  }
}
process.exitCode = interrupted === 'SIGINT' ? 130 : interrupted === 'SIGTERM' ? 143 : failed ? 1 : 0
