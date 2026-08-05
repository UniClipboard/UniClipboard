import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const rootDir = path.resolve(__dirname, '..')
const applicationPath =
  process.env.E2E_TAURI_APP ??
  path.join(
    rootDir,
    'target',
    'debug',
    process.platform === 'win32' ? 'uniclipboard.exe' : 'uniclipboard'
  )

function instance(profile, extraEnv = {}) {
  return {
    capabilities: {
      browserName: 'tauri',
      'tauri:options': {
        application: applicationPath,
      },
      'wdio:tauriServiceOptions': {
        appBinaryPath: applicationPath,
        driverProvider: 'embedded',
        startTimeout: 120000,
        env: {
          UNICLIPBOARD_ENV: process.env.UNICLIPBOARD_ENV ?? 'development',
          UC_PROFILE: profile,
          UC_DISABLE_SINGLE_INSTANCE: '1',
          UC_CLIPBOARD_MODE: 'passive',
          ...extraEnv,
        },
      },
    },
  }
}

export const config = {
  runner: 'local',
  specs: [path.join(__dirname, 'specs', 'setup-dual-peer.e2e.js')],
  maxInstances: 1,
  logLevel: 'error',
  waitforTimeout: 30000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 1,
  services: [
    [
      '@wdio/tauri-service',
      {
        appBinaryPath: applicationPath,
        driverProvider: 'embedded',
        startTimeout: 120000,
      },
    ],
  ],
  capabilities: {
    sponsor: instance(process.env.E2E_UC_SPONSOR_PROFILE ?? 'wdio-sponsor'),
    joiner: instance(process.env.E2E_UC_JOINER_PROFILE ?? 'wdio-joiner', {
      UC_DISABLE_SYSTEM_CLIPBOARD: '1',
    }),
  },
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 180000,
  },
}
