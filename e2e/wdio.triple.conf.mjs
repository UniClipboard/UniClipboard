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

function instance(profile) {
  return {
    capabilities: {
      browserName: 'tauri',
      'tauri:options': { application: applicationPath },
      'wdio:tauriServiceOptions': {
        appBinaryPath: applicationPath,
        driverProvider: 'embedded',
        startTimeout: 120000,
        env: {
          UNICLIPBOARD_ENV: process.env.UNICLIPBOARD_ENV ?? 'development',
          UC_PROFILE: profile,
          UC_DISABLE_SINGLE_INSTANCE: '1',
          UC_DISABLE_SYSTEM_CLIPBOARD: '1',
          UC_CLIPBOARD_MODE: 'passive',
        },
      },
    },
  }
}

export const config = {
  runner: 'local',
  specs: [path.join(__dirname, 'specs', '*.triple.e2e.js')],
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
    retained: instance(process.env.E2E_UC_RETAINED_PROFILE ?? 'wdio-retained'),
    removed: instance(process.env.E2E_UC_REMOVED_PROFILE ?? 'wdio-removed'),
  },
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: {
    ui: 'bdd',
    timeout: 240000,
  },
}
