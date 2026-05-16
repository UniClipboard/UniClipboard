import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(__dirname, '..')

function parsePortEnv(name, fallback) {
  const raw = process.env[name]
  if (raw == null || raw === '') return fallback
  const n = Number(raw)
  if (!Number.isInteger(n) || n <= 0 || n > 65535) {
    throw new Error(`${name} 不是合法端口（1-65535 整数）：${raw}`)
  }
  return n
}

const webdriverPort = parsePortEnv('E2E_WEBDRIVER_PORT', 4444)
const nativeWebdriverPort = parsePortEnv('E2E_NATIVE_WEBDRIVER_PORT', 4445)
const profile = process.env.E2E_UC_PROFILE ?? 'wdio'
const applicationPath =
  process.env.E2E_TAURI_APP ??
  path.join(
    rootDir,
    'src-tauri',
    'target',
    'debug',
    process.platform === 'win32' ? 'uniclipboard.exe' : 'uniclipboard'
  )
const isSupportedPlatform = process.platform === 'linux' || process.platform === 'win32'

let tauriDriverProcess
let closingTauriDriver = false

Object.assign(process.env, {
  UNICLIPBOARD_ENV: process.env.UNICLIPBOARD_ENV ?? 'development',
  UC_PROFILE: profile,
  UC_DISABLE_SINGLE_INSTANCE: process.env.UC_DISABLE_SINGLE_INSTANCE ?? '1',
  UC_CLIPBOARD_MODE: process.env.UC_CLIPBOARD_MODE ?? 'passive',
})

function assertSupportedPlatform() {
  if (isSupportedPlatform) return

  throw new Error(
    `tauri-driver 真窗口测试只支持 Linux/Windows；当前平台是 ${process.platform}。请在 Ubuntu 或 Windows 环境运行 bun run test:e2e:desktop。`
  )
}

assertSupportedPlatform()

function resolveProfileDataDir() {
  if (process.platform === 'win32') {
    return path.join(
      process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'),
      `app.uniclipboard.desktop-${profile}`
    )
  }

  if (process.platform === 'darwin') {
    return path.join(
      os.homedir(),
      'Library',
      'Application Support',
      `app.uniclipboard.desktop-${profile}`
    )
  }

  return path.join(
    process.env.XDG_DATA_HOME ?? path.join(os.homedir(), '.local', 'share'),
    `app.uniclipboard.desktop-${profile}`
  )
}

function cleanProfileData() {
  if (process.env.E2E_KEEP_PROFILE === '1') return

  fs.rmSync(resolveProfileDataDir(), { recursive: true, force: true })
}

function ensureApplicationBuilt() {
  if (process.env.E2E_SKIP_BUILD === '1') return

  const result = spawnSync('bun', ['run', 'tauri', 'build', '--debug', '--no-bundle'], {
    cwd: rootDir,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })

  if (result.status !== 0) {
    throw new Error(`Tauri 调试应用构建失败，退出码 ${result.status ?? 'unknown'}`)
  }
}

function assertApplicationExists() {
  if (fs.existsSync(applicationPath)) return

  throw new Error(`找不到 Tauri 调试应用：${applicationPath}`)
}

function resolveTauriDriverBinary() {
  if (process.env.TAURI_DRIVER) return process.env.TAURI_DRIVER

  const binary = process.platform === 'win32' ? 'tauri-driver.exe' : 'tauri-driver'
  const cargoPath = path.join(os.homedir(), '.cargo', 'bin', binary)
  return fs.existsSync(cargoPath) ? cargoPath : binary
}

function startTauriDriver() {
  const args = [`--port=${webdriverPort}`, `--native-port=${nativeWebdriverPort}`]
  tauriDriverProcess = spawn(resolveTauriDriverBinary(), args, {
    stdio: ['ignore', 'inherit', 'inherit'],
  })

  tauriDriverProcess.on('error', error => {
    console.error('tauri-driver 启动失败:', error)
    process.exit(1)
  })

  tauriDriverProcess.on('exit', code => {
    if (closingTauriDriver) return

    console.error(`tauri-driver 提前退出，退出码 ${code}`)
    process.exit(1)
  })
}

function closeTauriDriver() {
  closingTauriDriver = true
  tauriDriverProcess?.kill()
}

function waitForPortReady(port, { host = '127.0.0.1', timeoutMs = 10000, intervalMs = 100 } = {}) {
  const deadline = Date.now() + timeoutMs
  return new Promise((resolve, reject) => {
    const tryConnect = () => {
      const socket = net.createConnection({ port, host })
      socket.once('connect', () => {
        socket.end()
        resolve()
      })
      socket.once('error', () => {
        socket.destroy()
        if (Date.now() >= deadline) {
          reject(new Error(`tauri-driver 未在 ${timeoutMs}ms 内就绪：${host}:${port}`))
          return
        }
        setTimeout(tryConnect, intervalMs)
      })
    }
    tryConnect()
  })
}

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.once(signal, () => {
    closeTauriDriver()
    process.exit(1)
  })
}

export const config = {
  hostname: '127.0.0.1',
  port: webdriverPort,
  specs: [path.join(__dirname, 'specs', '**', '*.e2e.js')],
  maxInstances: 1,
  logLevel: 'warn',
  waitforTimeout: 30000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 1,
  capabilities: [
    {
      maxInstances: 1,
      'tauri:options': {
        application: applicationPath,
        args: [],
      },
    },
  ],
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 90000,
  },
  onPrepare() {
    assertSupportedPlatform()
    cleanProfileData()
    ensureApplicationBuilt()
    assertApplicationExists()
  },
  async beforeSession() {
    startTauriDriver()
    await waitForPortReady(webdriverPort)
  },
  afterSession() {
    closeTauriDriver()
  },
  onComplete() {
    closeTauriDriver()
  },
}
