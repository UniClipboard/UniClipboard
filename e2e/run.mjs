import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(__dirname, '..')
const dualPeerMode = process.argv.includes('--dual-peer')
const profile = process.env.E2E_UC_PROFILE ?? 'wdio'
const profiles = dualPeerMode
  ? [
      process.env.E2E_UC_SPONSOR_PROFILE ?? 'wdio-sponsor',
      process.env.E2E_UC_JOINER_PROFILE ?? 'wdio-joiner',
    ]
  : [profile]
const wdioConfig = dualPeerMode ? 'e2e/wdio.dual.conf.mjs' : 'e2e/wdio.conf.mjs'
const forwardedArgs = process.argv.slice(2).filter(arg => arg !== '--dual-peer')
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

function run(command, args, failureMessage) {
  const result = spawnSync(command, args, {
    cwd: rootDir,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })

  if (result.status !== 0) {
    throw new Error(`${failureMessage}，退出码 ${result.status ?? 'unknown'}`)
  }
}

if (process.env.E2E_KEEP_PROFILE !== '1') {
  for (const activeProfile of profiles) {
    fs.rmSync(resolveProfileDataDir(activeProfile), { recursive: true, force: true })
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

run('bunx', ['wdio', 'run', wdioConfig, ...forwardedArgs], 'Tauri E2E 测试失败')
