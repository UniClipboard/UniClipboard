import { readFileSync } from 'node:fs'
import path from 'node:path'
import { validateTestProfile } from './helpers/conflict-userdata.mjs'

const run = JSON.parse(readFileSync(process.env.CONFLICT_RUN, 'utf8'))
const application = path.resolve('target/debug/uniclipboard')
const instance = profile => ({
  capabilities: {
    browserName: 'tauri',
    'tauri:options': { application },
    'wdio:tauriServiceOptions': {
      appBinaryPath: application,
      driverProvider: 'embedded',
      startTimeout: 120000,
      env: {
        ...process.env,
        UC_PROFILE: validateTestProfile(profile),
        UNICLIPBOARD_ENV: 'development',
        UC_DISABLE_SINGLE_INSTANCE: '1',
        UC_DISABLE_SYSTEM_CLIPBOARD: '1',
        UC_CLIPBOARD_MODE: 'passive',
      },
    },
  },
})

export const config = {
  runner: 'local',
  specs: [path.resolve('e2e/specs/device-group-conflict.e2e.js')],
  maxInstances: 1,
  logLevel: 'error',
  waitforTimeout: 30000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 0,
  services: [
    [
      '@wdio/tauri-service',
      { appBinaryPath: application, driverProvider: 'embedded', startTimeout: 120000 },
    ],
  ],
  capabilities: Object.fromEntries(
    run.profiles
      .map((profile, index) => ['abcde'[index], instance(profile)])
      .filter(([role]) => process.env.CONFLICT_CASE !== 'native-keyboard' || role === 'd')
  ),
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: { ui: 'bdd', timeout: 300000 },
  afterTest: async function (_test, _context, { passed }) {
    if (passed) return
    const { browser } = await import('@wdio/globals')
    const { mkdir, writeFile } = await import('node:fs/promises')
    const { connect } = await import('./helpers/conflict-userdata.mjs')
    const { describeGroups, screenshot } = await import('./helpers/conflict-gui.js')
    const { collectEngineEvidence } = await import('./helpers/conflict-evidence.mjs')
    const diagnostics = []
    for (const [index, profile] of run.profiles.entries()) {
      try {
        const connection = await connect(profile)
        const entries = []
        for (const route of [
          '/encryption/state',
          '/member/device-group-choices',
          '/member/protection',
        ]) {
          const response = await fetch(`${connection.base}${route}`, {
            headers: { Authorization: `Session ${connection.sessionToken}` },
            signal: AbortSignal.timeout(3000),
          })
          const value = await response.json()
          entries.push({
            route,
            status: response.status,
            code: value.code,
            initialized: value.data?.initialized,
            sessionReady: value.data?.sessionReady,
            issueCount: value.data?.issues?.length,
            mode: value.data?.mode,
            groups: value.data?.deviceTrust ? describeGroups(value.data, run) : undefined,
          })
        }
        diagnostics.push({ role: 'abcde'[index], entries })
      } catch {
        diagnostics.push({ role: 'abcde'[index], unavailable: true })
      }
    }
    await writeFile(
      path.join(run.runDir, 'failure-diagnostics.json'),
      JSON.stringify(diagnostics, null, 2),
      { mode: 0o600 }
    )
    await collectEngineEvidence(run)
    await mkdir(path.join(run.runDir, 'screenshots'), { recursive: true })
    for (const role of browser.instances ?? [])
      try {
        await screenshot(browser[role], run, `failure-${role}`)
      } catch {}
  },
}
