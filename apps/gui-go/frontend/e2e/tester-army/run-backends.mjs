import { spawn } from 'node:child_process'
import { mkdir, rm, writeFile } from 'node:fs/promises'
import { backendSuites, verifiedBackendRuns, retainedBackendReasons } from './backend-suites.mjs'

const args = process.argv.slice(2)
const named = []
let platform, seed, phase
for (let index = 0; index < args.length; index++) {
  const arg = args[index]
  if (arg === '--platform') platform = args[++index]
  else if (arg === '--seed') seed = args[++index]
  else if (arg === '--phase') phase = args[++index]
  else if (arg.startsWith('--')) throw new Error(`Unknown option ${arg}`)
  else named.push(arg)
}
const selection = []
if (!named.length) {
  selection.push(
    ...verifiedBackendRuns.filter(
      run =>
        (!platform || run.platform === platform) &&
        (!seed || run.seed === seed) &&
        (!phase || run.phase === phase)
    )
  )
  for (const [name, reason] of Object.entries(retainedBackendReasons))
    console.log(`RETAINED ${name}: ${reason}`)
} else {
  for (const suite of named) {
    const options = backendSuites[suite]
    if (!options) throw new Error(`Unknown backend suite ${suite}`)
    for (const skin of platform ? [platform] : options.platforms)
      for (const data of seed ? [seed] : options.seeds)
        selection.push({ suite, platform: skin, seed: data, phase })
  }
}
if (selection.length === 0) throw new Error('No backend runs selected')
for (const run of selection) {
  if (
    !backendSuites[run.suite].platforms.includes(run.platform) ||
    !backendSuites[run.suite].seeds.includes(run.seed)
  )
    throw new Error('Unsupported backend selection')
  if (
    run.phase &&
    (run.suite !== 'history-search' ||
      !['components', 'fullPage', 'fullPageWindows', 'fullPageLinuxSystemFrame'].includes(
        run.phase
      ))
  )
    throw new Error('Unknown backend phase')
}
const runs = []
await mkdir('target/tester-army-backend-report', { recursive: true })
let active, interrupted
for (const signal of ['SIGINT', 'SIGTERM'])
  process.once(signal, () => {
    interrupted = signal
    active?.kill(signal)
  })
let failed = false
for (const run of selection) {
  if (interrupted) break
  const folder = `${run.platform}/${run.suite}/${run.seed}/${run.phase || 'all'}`
  await rm(`target/tester-army-backend/${folder}`, { recursive: true, force: true })
  await rm(`target/tester-army-backend-report/${folder}`, { recursive: true, force: true })
  const child = spawn(
    process.execPath,
    ['node_modules/e2e/dist/cli/bin.js', 'run', '--config', 'e2e.backend.config.ts'],
    {
      stdio: 'inherit',
      env: {
        ...process.env,
        E2E_TELEMETRY_DISABLED: '1',
        UC_TESTER_BACKEND_SUITE: run.suite,
        UC_TESTER_PLATFORM: run.platform,
        UC_TESTER_BACKEND_SEED: run.seed,
        UC_TESTER_BACKEND_PHASE: run.phase || '',
        DEVICES_KIT_LIFECYCLE: '1',
        DEVICES_KIT_PASSPHRASE_LIFECYCLE: '1',
      },
    }
  )
  active = child
  const code = await new Promise((resolve, reject) => {
    child.once('error', reject)
    child.once('exit', resolve)
  })
  active = undefined
  runs.push({ ...run, phase: run.phase || 'all-original-phases', exitCode: code })
  await writeFile(
    'target/tester-army-backend-report/selection.json',
    JSON.stringify(
      {
        scope: named.length ? 'explicit-candidates' : 'verified-combinations',
        runs,
        retained: retainedBackendReasons,
      },
      null,
      2
    ) + '\n'
  )
  if (code !== 0) failed = true
}
process.exitCode = interrupted ? 130 : failed ? 1 : 0
