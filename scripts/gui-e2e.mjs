import { spawn, execFileSync } from 'node:child_process'
import { cp, mkdir, rm, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  allocateBuildLease,
  releaseBuildLease,
  buildLeaseEnv,
} from '../apps/gui-go/frontend/e2e/tester-army/build-lease.mjs'
import {
  suites,
  verifiedPlatforms,
  retainedReasons,
  backendSuites,
  verifiedBackendRuns,
  retainedBackendReasons,
} from '../apps/gui-go/frontend/e2e/tester-army/catalog.mjs'

const root = fileURLToPath(new URL('../', import.meta.url))
const invocation = process.cwd()
const [group, ...args] = process.argv.slice(2)
const sdk = ['node_modules/e2e/dist/cli/bin.js', 'run']
const singles = {
  browser: [...sdk],
  daemon: [...sdk, '--config', 'e2e.daemon.config.ts'],
  'native-macos-history': ['apps/gui-go/e2e/tester-army-native/run-macos-history.mjs'],
  'native-macos-file-transfer': [
    'apps/gui-go/e2e/tester-army-native/run-macos-history.mjs',
    'file-transfer',
  ],
  'native-windows-product': ['apps/gui-go/e2e/windows_startup_ready.mjs', '--tester-army'],
}
if (!group || ['--help', '-h'].includes(group)) {
  console.log(
    'Usage: bun run e2e:gui list|browser|daemon|layouts|backends|native-macos-history|native-macos-file-transfer|native-windows-product [suite] [--platform skin] [--seed seed] [--phase phase] [--out new-directory]'
  )
  process.exit(0)
}
if (group === 'list') {
  if (args.length) throw new Error('list takes no arguments')
  console.log(
    JSON.stringify(
      {
        browser: ['custom-relays', 'relay-overview'],
        layouts: { candidates: suites, verified: verifiedPlatforms, retained: retainedReasons },
        backends: {
          candidates: backendSuites,
          verified: verifiedBackendRuns,
          retained: retainedBackendReasons,
          host: 'darwin',
        },
        native: {
          'native-macos-history': 'darwin; real AX product acceptance',
          'native-macos-file-transfer':
            'darwin; native AX file progress and real paired Engine transfer',
          'native-windows-product': 'win32; prebuilt real WebView2 product',
        },
        limits:
          'Skin emulation and runtime capability probes do not establish native product acceptance.',
      },
      null,
      2
    )
  )
  process.exit(0)
}
if (!['layouts', 'backends', ...Object.keys(singles)].includes(group))
  throw new Error(`Unknown GUI E2E group: ${group}`)

let selection, output, scope, retained, out
const matrix = ['layouts', 'backends'].includes(group)
if (matrix) {
  const names = []
  const options = {}
  for (let index = 0; index < args.length; index++) {
    const arg = args[index]
    if (arg.startsWith('--')) {
      const separator = arg.indexOf('=')
      const flag = separator < 0 ? arg : arg.slice(0, separator)
      const inline = separator < 0 ? undefined : arg.slice(separator + 1)
      const allowed =
        group === 'layouts' ? ['--platform', '--out'] : ['--platform', '--seed', '--phase']
      if (!allowed.includes(flag) || Object.hasOwn(options, flag))
        throw new Error(`Unsupported or repeated option: ${flag}`)
      const value = inline ?? args[++index]
      if (!value || value.startsWith('--')) throw new Error(`${flag} requires a value`)
      options[flag] = value
    } else names.push(arg)
  }
  if (new Set(names).size !== names.length) throw new Error('Repeated suite selection')
  const platform = options['--platform'],
    seed = options['--seed'],
    phase = options['--phase']
  if (group === 'layouts') {
    selection = []
    for (const suite of names.length ? names : Object.keys(verifiedPlatforms)) {
      if (!suites[suite]) throw new Error(`Unknown layout suite: ${suite}`)
      if (platform && !suites[suite].platforms.includes(platform)) {
        if (names.length) throw new Error(`Unsupported skin ${platform} for ${suite}`)
        continue
      }
      for (const skin of names.length ? suites[suite].platforms : verifiedPlatforms[suite])
        if (!platform || skin === platform) selection.push({ suite, platform: skin })
    }
    retained = retainedReasons
    scope = names.length ? 'explicit-candidate' : 'verified-combinations'
    output = 'target/tester-army-layout-report'
    if (options['--out']) {
      if (names.length !== 1) throw new Error('--out requires exactly one explicit layout suite')
      out = path.resolve(invocation, options['--out'])
      for (const run of selection) {
        for (const folder of ['tester-army-layout', 'tester-army-layout-report']) {
          const source = path.join(root, 'target', folder, run.platform, run.suite)
          if (
            out === source ||
            out.startsWith(source + path.sep) ||
            source.startsWith(out + path.sep)
          )
            throw new Error('--out must not overlap canonical artifacts')
        }
      }
      try {
        await stat(out)
        throw new Error('--out requires a new directory')
      } catch (error) {
        if (error.code !== 'ENOENT') throw error
      }
    }
  } else {
    if (process.platform !== 'darwin')
      throw new Error('Isolated backend acceptance currently requires macOS')
    selection = names.length
      ? names.flatMap(suite => {
          const item = backendSuites[suite]
          if (!item) throw new Error(`Unknown backend suite: ${suite}`)
          return (platform ? [platform] : item.platforms).flatMap(skin =>
            (seed ? [seed] : item.seeds).map(data => ({
              suite,
              platform: skin,
              seed: data,
              ...(phase ? { phase } : {}),
            }))
          )
        })
      : verifiedBackendRuns.filter(
          run =>
            (!platform || run.platform === platform) &&
            (!seed || run.seed === seed) &&
            (!phase || run.phase === phase)
        )
    for (const run of selection) {
      if (
        !backendSuites[run.suite].platforms.includes(run.platform) ||
        !backendSuites[run.suite].seeds.includes(run.seed)
      )
        throw new Error('Unsupported backend skin/seed')
      if (
        run.phase &&
        (run.suite !== 'history-search' ||
          !['components', 'fullPage', 'fullPageWindows', 'fullPageLinuxSystemFrame'].includes(
            run.phase
          ))
      )
        throw new Error('Unsupported backend phase')
    }
    retained = retainedBackendReasons
    scope = names.length ? 'explicit-candidates' : 'verified-combinations'
    output = 'target/tester-army-backend-report'
  }
  if (!selection.length) throw new Error('No GUI E2E runs selected')
  if (!names.length)
    for (const [name, reason] of Object.entries(retained))
      console.log(`RETAINED ${name}: ${reason}`)
} else {
  if (
    ['native-macos-history', 'native-macos-file-transfer'].includes(group) &&
    (process.platform !== 'darwin' || args.length)
  )
    throw new Error(`${group} requires macOS and takes no arguments`)
  if (
    group === 'native-windows-product' &&
    (process.platform !== 'win32' || args.some(arg => arg !== '--long-path'))
  )
    throw new Error('native-windows-product requires Windows; only --long-path is supported')
  if (args.some(arg => /^--config(?:=|$)/.test(arg)))
    throw new Error('Select a GUI E2E group instead of overriding its config')
  selection = [{}]
}
process.chdir(root)
const sourceHead = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
let active, interrupted
const handlers = new Map(
  ['SIGINT', 'SIGTERM'].map(signal => [
    signal,
    () => {
      if (interrupted) return
      interrupted = signal
      if (active && active.exitCode === null && active.signalCode === null) active.kill(signal)
    },
  ])
)
for (const [signal, handler] of handlers) process.on(signal, handler)
const runs = []
const receipt = matrix
  ? {
      head: sourceHead,
      dirty: execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() !== '',
      node: process.versions.node,
      host: process.platform,
      scope,
      runs,
      retained,
    }
  : undefined
async function save() {
  if (!matrix) return
  await mkdir(output, { recursive: true })
  await writeFile(
    `${output}/selection.json`,
    JSON.stringify({ ...receipt, ...(interrupted ? { interrupted } : {}) }, null, 2) + '\n'
  )
}
let failed = false
try {
  if (['browser', 'daemon'].includes(group) && !args.some(arg => ['--help', '-h'].includes(arg)))
    await rm(group === 'browser' ? 'target/tester-army' : 'target/tester-army-daemon', {
      recursive: true,
      force: true,
    })
  await save()
  for (const run of selection) {
    if (interrupted) break
    let command = singles[group],
      env = { ...process.env, E2E_TELEMETRY_DISABLED: '1' }
    if (matrix) {
      const backend = group === 'backends'
      const folder = backend
        ? `${run.platform}/${run.suite}/${run.seed}/${run.phase || 'all'}`
        : `${run.platform}/${run.suite}`
      await rm(`target/tester-army-${backend ? 'backend' : 'layout'}/${folder}`, {
        recursive: true,
        force: true,
      })
      await rm(`${output}/${folder}`, { recursive: true, force: true })
      command = [...sdk, '--config', backend ? 'e2e.backend.config.ts' : 'e2e.layouts.config.ts']
      for (const key of ['UC_UI_PLATFORM', 'UC_TESTER_DAEMON', 'UC_TESTER_BACKENDS'])
        delete env[key]
      env = {
        ...env,
        UC_TESTER_PLATFORM: run.platform,
        ...(backend
          ? {
              UC_TESTER_BACKEND_SUITE: run.suite,
              UC_TESTER_BACKEND_SEED: run.seed,
              UC_TESTER_BACKEND_PHASE: run.phase || '',
              DEVICES_KIT_LIFECYCLE: '1',
              DEVICES_KIT_PASSPHRASE_LIFECYCLE: '1',
            }
          : { UC_TESTER_SUITE: run.suite }),
      }
    }
    const lease =
      ['layouts', 'backends', 'browser', 'daemon'].includes(group) &&
      !args.some(arg => ['--help', '-h'].includes(arg))
        ? await allocateBuildLease()
        : undefined
    if (lease) env[buildLeaseEnv] = lease
    let result
    try {
      if (interrupted) break
      const child = spawn(process.execPath, [...command, ...(matrix ? [] : args)], {
        cwd: root,
        env,
        stdio: 'inherit',
      })
      active = child
      result = await new Promise(resolve => {
        let errorCode
        child.once('error', error => {
          errorCode = error.code || error.name
        })
        child.once('close', (exitCode, signal) =>
          resolve({ exitCode, ...(errorCode ? { errorCode } : {}), ...(signal ? { signal } : {}) })
        )
      })
    } finally {
      active = undefined
      if (lease) {
        const cleanup = await releaseBuildLease(lease)
        const reportDirectory = matrix
          ? `${output}/${run.platform}/${run.suite}${group === 'backends' ? `/${run.seed}/${run.phase || 'all'}` : ''}`
          : group === 'browser'
            ? 'target/tester-army'
            : 'target/tester-army-daemon'
        await mkdir(reportDirectory, { recursive: true })
        await writeFile(
          `${reportDirectory}/fixture-cleanup.json`,
          JSON.stringify({ head: sourceHead, ...cleanup }, null, 2) + '\n'
        )
        if (result) result.buildCleanupPassed = cleanup.passed
        if (!cleanup.passed) failed = true
      }
    }
    runs.push({
      ...run,
      ...(group === 'backends' ? { phase: run.phase || 'all-original-phases' } : {}),
      ...result,
    })
    await save()
    if (result.exitCode !== 0) failed = true
  }
  if (
    matrix &&
    execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim() !== receipt.head
  )
    throw new Error('Source HEAD changed during GUI E2E execution')
  if (out && !failed && !interrupted) {
    for (const run of selection)
      await cp(
        `target/tester-army-layout/${run.platform}/${run.suite}`,
        selection.length === 1 ? out : path.join(out, run.platform),
        { recursive: true, force: false, errorOnExist: true }
      )
  }
} finally {
  await save()
  for (const [signal, handler] of handlers) process.removeListener(signal, handler)
}
process.exitCode = interrupted === 'SIGINT' ? 130 : interrupted === 'SIGTERM' ? 143 : failed ? 1 : 0
