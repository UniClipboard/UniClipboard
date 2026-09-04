import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { createSpecRuns } from './run-plan.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(__dirname, '..')
const dualPeerMode = process.argv.includes('--dual-peer')
const triplePeerMode = process.argv.includes('--triple-peer')
if (dualPeerMode && triplePeerMode) {
  throw new Error('双设备和三设备模式不能同时启用')
}
const profile = process.env.E2E_UC_PROFILE ?? 'wdio'
const wdioConfig = triplePeerMode
  ? 'e2e/wdio.triple.conf.mjs'
  : dualPeerMode
    ? 'e2e/wdio.dual.conf.mjs'
    : 'e2e/wdio.conf.mjs'
const forwardedArgs = process.argv
  .slice(2)
  .filter(arg => arg !== '--dual-peer' && arg !== '--triple-peer')
const requestedSpec = process.env.E2E_SPEC
const specFiles = fs
  .readdirSync(path.join(__dirname, 'specs'))
  .filter(name => name.endsWith('.e2e.js'))
  .filter(name => {
    if (triplePeerMode) return name.endsWith('.triple.e2e.js')
    if (dualPeerMode) return name.endsWith('.dual.e2e.js')
    return !name.endsWith('.dual.e2e.js') && !name.endsWith('.triple.e2e.js')
  })
  .map(name => path.join(__dirname, 'specs', name))
  .filter(spec => !requestedSpec || spec === path.resolve(rootDir, requestedSpec))
  .sort()
const specRuns = createSpecRuns({
  specs: specFiles,
  dualPeerMode,
  triplePeerMode,
  profile,
  sponsorProfile: process.env.E2E_UC_SPONSOR_PROFILE ?? 'wdio-sponsor',
  joinerProfile: process.env.E2E_UC_JOINER_PROFILE ?? 'wdio-joiner',
  retainedProfile: process.env.E2E_UC_RETAINED_PROFILE ?? 'wdio-retained',
  removedProfile: process.env.E2E_UC_REMOVED_PROFILE ?? 'wdio-removed',
  hostPlatform: process.platform,
  hostArch: process.arch,
})
const applicationPath =
  process.env.E2E_TAURI_APP ??
  path.join(
    rootDir,
    'target',
    'debug',
    process.platform === 'win32' ? 'uniclipboard.exe' : 'uniclipboard'
  )

function resolveProfileDataDir(activeProfile) {
  if (process.platform === 'win32') {
    return path.join(
      process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'),
      `app.uniclipboard.desktop-${activeProfile}`
    )
  }

  if (process.platform === 'darwin') {
    return path.join(
      os.homedir(),
      'Library',
      'Application Support',
      `app.uniclipboard.desktop-${activeProfile}`
    )
  }

  return path.join(
    process.env.XDG_DATA_HOME ?? path.join(os.homedir(), '.local', 'share'),
    `app.uniclipboard.desktop-${activeProfile}`
  )
}

function run(command, args, failureMessage, env = process.env) {
  const result = spawnSync(command, args, {
    cwd: rootDir,
    env,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })

  if (result.status !== 0) {
    throw new Error(`${failureMessage}，退出码 ${result.status ?? 'unknown'}`)
  }
}

if (process.env.E2E_SKIP_BUILD !== '1') {
  run('bun', ['run', 'daemon:dev'], 'Tauri E2E 后台程序构建失败')
  run(
    'bun',
    [
      'run',
      'tauri',
      'build',
      '--debug',
      '--no-bundle',
      '--features',
      'e2e',
      '--config',
      'src-tauri/tauri.e2e.conf.json',
    ],
    'Tauri E2E 应用构建失败'
  )
}

if (!fs.existsSync(applicationPath)) {
  throw new Error(`找不到 Tauri E2E 应用：${applicationPath}`)
}

if (specRuns.length === 0) {
  throw new Error('没有找到可运行的 Tauri E2E 场景')
}

for (const specRun of specRuns) {
  if (process.env.E2E_KEEP_PROFILE !== '1') {
    for (const activeProfile of specRun.profiles) {
      fs.rmSync(resolveProfileDataDir(activeProfile), { recursive: true, force: true })
    }
  }
  if (specRun.fixture && process.env.E2E_UPGRADE_REPAIR_CLEARED !== '1') {
    run(
      'cargo',
      [
        'run',
        '--locked',
        '--manifest-path',
        'tests/e2e/Cargo.toml',
        '--bin',
        'restore-upgrade-userdata-fixture',
        '--',
        '--fixture',
        specRun.fixture.directory,
        '--profile',
        specRun.fixture.profile,
      ],
      `升级 userdata 样本恢复失败：${path.basename(specRun.spec)}`
    )
  }
  run(
    'bunx',
    ['wdio', 'run', wdioConfig, '--spec', specRun.spec, ...forwardedArgs],
    `Tauri E2E 测试失败：${path.basename(specRun.spec)}`,
    { ...process.env, ...specRun.env }
  )
}
