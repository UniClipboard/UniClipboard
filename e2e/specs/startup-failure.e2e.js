import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
import { $, $$, browser, expect } from '@wdio/globals'
import { confirmNativeSaveDialog } from '../helpers/native-save-dialog.mjs'

const profile = process.env.E2E_UC_PROFILE
const scenario = process.env.E2E_STARTUP_SCENARIO
const artifacts = path.resolve('e2e/artifacts', profile)
const exportDir = path.join(artifacts, 'exports')
const dataDir = path.join(
  homedir(),
  'Library',
  'Application Support',
  `app.uniclipboard.desktop-${profile}`
)
const logsDir = path.join(homedir(), 'Library', 'Logs', `app.uniclipboard.desktop-${profile}`)

const keyMissing = scenario === 'upgrade-backup-key-missing'

async function bodyText() {
  return $('body').getText()
}

async function buttonLabels() {
  return Promise.all((await $$('button')).map(button => button.getText()))
}

function exportedArchives() {
  return fs.existsSync(exportDir) ? fs.readdirSync(exportDir).filter(n => n.endsWith('.zip')) : []
}

async function exportThroughNativeDialog(label = '导出诊断记录') {
  const before = exportedArchives().length
  const button = await $(`button*=${label}`)
  await button.waitForClickable({ timeout: 10000 })
  await browser.execute(element => element.click(), button)
  confirmNativeSaveDialog({ processName: 'uniclipboard', directory: exportDir })
  return before
}

function readArchive(name, entry) {
  return execFileSync('unzip', ['-p', path.join(exportDir, name), entry], { encoding: 'utf8' })
}

function listArchive(name) {
  return execFileSync('unzip', ['-Z1', path.join(exportDir, name)], { encoding: 'utf8' })
    .split('\n')
    .filter(Boolean)
}

function daemonPid() {
  const conn = JSON.parse(fs.readFileSync(path.join(dataDir, 'daemon-startup.conn'), 'utf8'))
  return conn.pid
}

describe(`startup failure page: ${scenario}`, () => {
  before(() => fs.mkdirSync(exportDir, { recursive: true }))

  afterEach(async function () {
    await browser.saveScreenshot(
      path.join(artifacts, `${this.currentTest?.title.slice(0, 40)}.png`)
    )
  })

  it('shows an understandable failure page for the Engine start result', async () => {
    await browser.waitUntil(async () => (await bodyText()).includes('应用未能启动'), {
      timeout: 90000,
      timeoutMsg: 'the startup failure page did not appear',
    })
    const text = await bodyText()
    const labels = await buttonLabels()
    fs.writeFileSync(path.join(artifacts, 'page-text.txt'), `${text}\n\n${labels.join(' | ')}\n`)
    expect(text).not.toContain('daemon startup did not complete')
    expect(labels.some(label => label.includes('导出诊断记录'))).toBe(true)
    if (keyMissing) {
      // States what is missing without claiming the user data is gone, and offers no retry.
      expect(text).toContain('升级备份的安全记录需要一把密钥才能解密')
      expect(text).toContain('并不表示剪贴板内容或其他用户资料已经丢失')
      expect(labels.some(label => label.includes('重试'))).toBe(false)
    } else {
      // Any other startup error keeps the generic message and the retry action.
      expect(text).toContain('应用未能完成启动')
      expect(labels.some(label => label.includes('重试'))).toBe(true)
    }
  })

  it('exports diagnostics while the daemon reports the failure', async () => {
    const before = await exportThroughNativeDialog()
    await browser.waitUntil(() => exportedArchives().length > before, {
      timeout: 30000,
      timeoutMsg: 'the diagnostic archive was not written',
    })
    const [name] = exportedArchives()
    const entries = listArchive(name)
    fs.writeFileSync(path.join(artifacts, 'archive-entries-live.txt'), `${entries.join('\n')}\n`)
    expect(entries).toContain('manifest.json')
    expect(entries.some(entry => entry.startsWith('logs/uniclipboard-daemon'))).toBe(true)
    const manifest = JSON.parse(readArchive(name, 'manifest.json'))
    const failure = manifest.startupStatus?.progress?.failure
    expect(failure?.reason).toBe(keyMissing ? 'upgrade_backup_key_missing' : 'backup_failed')
    expect(failure?.retryable).toBe(!keyMissing)
    await $('*=诊断记录已导出').waitForExist({ timeout: 10000 })
  })

  it('keeps the page and still exports existing logs after the daemon exits', async () => {
    process.kill(daemonPid(), 'SIGTERM')
    await browser.pause(4000)
    const text = await bodyText()
    expect(text).toContain('应用未能启动')
    expect(text).not.toContain('daemon startup did not complete')
    for (const name of exportedArchives()) fs.rmSync(path.join(exportDir, name))
    const before = await exportThroughNativeDialog()
    await browser.waitUntil(() => exportedArchives().length > before, { timeout: 30000 })
    const [name] = exportedArchives()
    const entries = listArchive(name)
    fs.writeFileSync(path.join(artifacts, 'archive-entries-offline.txt'), `${entries.join('\n')}\n`)
    expect(entries.some(entry => entry.startsWith('logs/uniclipboard-daemon'))).toBe(true)
  })

  it('explains an export failure with a way forward', async () => {
    fs.chmodSync(logsDir, 0o000)
    try {
      await exportThroughNativeDialog()
      const alert = await $('[role="alert"]')
      await alert.waitForExist({ timeout: 20000 })
      const text = await alert.getText()
      fs.writeFileSync(path.join(artifacts, 'export-failure-text.txt'), `${text}\n`)
      expect(text).toContain('导出失败')
      expect(text).toContain('github.com/UniClipboard/UniClipboard/issues')
    } finally {
      fs.chmodSync(logsDir, 0o755)
    }
  })
})
