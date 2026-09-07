import { spawnSync, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, rmdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { assertNativeKeyboardAvailable } from './helpers/conflict-native.mjs'
import { stopRunProfiles, testDaemons } from './helpers/conflict-userdata.mjs'

const baseline = process.argv[2] ?? 'four'
const scenario = process.argv[3] ?? 'apply'
const supported = [
  'apply',
  'keep',
  'cross-local',
  'cross-remote',
  'disagreement',
  'local-remove',
  'new-peer',
  'disconnect',
  'restart-cycle',
  'response-loss',
  'controlled',
  'native-keyboard',
  'recover-frozen',
]
if (!['four', 'five'].includes(baseline) || !supported.includes(scenario))
  throw new Error('Unsupported baseline or scenario')
if (scenario === 'new-peer' && baseline !== 'four') throw new Error('new-peer starts from four')
if (scenario === 'native-keyboard') assertNativeKeyboardAvailable()
function run(args, env = process.env, capture = false) {
  const result = spawnSync(process.execPath, args, {
    env,
    encoding: 'utf8',
    stdio: capture ? 'pipe' : 'inherit',
  })
  if (result.status !== 0) throw new Error(`Test step failed: ${args[0]}`)
  return result.stdout?.trim()
}
mkdirSync('.cache/device-group-e2e', { recursive: true })
mkdirSync('.cache/device-group-e2e/gui.lock')
try {
  if (testDaemons().length)
    throw new Error(
      'A previous conflict test daemon is still running; stop its run before restoring identities'
    )
  if (!existsSync(`.cache/device-group-e2e/${baseline}/manifest.json`))
    run(['e2e/conflict-userdata.mjs', 'generate', baseline])
  const restore =
    scenario === 'recover-frozen'
      ? ['e2e/conflict-userdata.mjs', 'restore-failed', baseline, process.argv[4] ?? '']
      : ['e2e/conflict-userdata.mjs', 'restore', baseline]
  const manifest = run(restore, process.env, true)
  console.log(`Run manifest: ${manifest}`)
  const provenance = {
    capturedAt: new Date().toISOString(),
    engine: execFileSync('git', ['-C', '../Engine', 'rev-parse', 'HEAD'], {
      encoding: 'utf8',
    }).trim(),
    engineStatus: execFileSync('git', ['-C', '../Engine', 'status', '--short'], {
      encoding: 'utf8',
    }),
    desktop: execFileSync('git', ['rev-parse', 'HEAD'], {
      encoding: 'utf8',
    }).trim(),
    desktopStatus: execFileSync('git', ['status', '--short'], {
      encoding: 'utf8',
    }),
    binaries: Object.fromEntries(
      ['uniclipboard', 'uniclipd'].map(name => [
        name,
        {
          path: resolve('target/debug', name),
          sha256: createHash('sha256')
            .update(readFileSync(resolve('target/debug', name)))
            .digest('hex'),
        },
      ])
    ),
  }
  writeFileSync(resolve(manifest, '../build.json'), JSON.stringify(provenance, null, 2), {
    mode: 0o600,
  })
  if (scenario === 'new-peer') {
    if (baseline !== 'four') throw new Error('new-peer starts from a four-device baseline')
    const data = JSON.parse(readFileSync(manifest, 'utf8'))
    data.profiles.push(data.profiles[0].replace(/-a$/, '-e'))
    data.expected.push(null)
    writeFileSync(manifest, JSON.stringify(data, null, 2), { mode: 0o600 })
  }
  try {
    const stages =
      scenario === 'restart-cycle'
        ? ['prepare-pending', 'resolve-restored', 'verify-resolved']
        : [scenario]
    for (const stage of stages) {
      run(['node_modules/@wdio/cli/bin/wdio.js', 'run', 'e2e/wdio.conflict.conf.mjs'], {
        ...process.env,
        CONFLICT_RUN: resolve(manifest),
        CONFLICT_CASE: stage,
      })
      await stopRunProfiles(JSON.parse(readFileSync(manifest, 'utf8')).profiles)
    }
  } finally {
    run(['e2e/conflict-userdata.mjs', 'stop', baseline, manifest])
  }
} finally {
  rmdirSync('.cache/device-group-e2e/gui.lock')
}
