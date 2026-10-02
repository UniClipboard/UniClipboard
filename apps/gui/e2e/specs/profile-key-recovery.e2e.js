import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'
import { daemonConnection, daemonRequest } from '../helpers/dualPeer.js'

function expectHistoryReadable(profile, marker) {
  const cli = path.resolve('target/debug/uniclip')
  const common = ['--dev', '--profile', profile]
  const env = { ...process.env, UC_PROFILE: profile, UNICLIPBOARD_ENV: 'development' }
  const result = JSON.parse(
    execFileSync(cli, [...common, 'search', marker, '--json'], { encoding: 'utf8', env })
  )
  const entry = result.data.find(candidate => candidate.text_preview === marker)
  expect(entry?.entry_id).toBeTruthy()
  expect(
    execFileSync(cli, [...common, 'get', '--id', entry.entry_id], { encoding: 'utf8', env })
  ).toBe(marker)
}

async function settleFiniteAnimations() {
  await browser.execute(() => {
    for (const animation of document.getAnimations())
      if (animation.effect?.getComputedTiming().iterations !== Infinity) animation.finish()
  })
}

async function enterHistory(passphrase) {
  await browser.waitUntil(
    async () =>
      (await $('button*=解锁').isDisplayed()) ||
      (await $('[data-testid="history-search-anchor"]').isDisplayed()),
    { timeout: 60000 }
  )
  if (await $('button*=解锁').isDisplayed()) {
    await $('button*=解锁').click()
    const password = await $('#unlock-passphrase')
    await browser.waitUntil(
      async () =>
        (await password.isDisplayed()) ||
        (await $('[data-testid="history-search-anchor"]').isDisplayed()),
      { timeout: 30000 }
    )
    if (await password.isDisplayed()) {
      await password.setValue(passphrase)
      await browser.keys('Enter')
    }
  }
  const dismiss = await $('button*=不再提示')
  if (await dismiss.isDisplayed()) await dismiss.click()
  await $('[data-testid="history-search-anchor"]').waitForDisplayed({ timeout: 60000 })
}

describe('profile key recovery', () => {
  afterEach(async function () {
    if (this.currentTest?.state !== 'failed') return
    const screenshots = path.resolve('e2e/artifacts', process.env.E2E_UC_PROFILE)
    fs.mkdirSync(screenshots, { recursive: true })
    await browser.saveScreenshot(path.join(screenshots, 'failed.png'))
    console.error('Recovery test visible state:', await $('body').getText())
  })
  it('recovers from missing keys without losing the original history', async () => {
    const passphrase = process.env.E2E_UNLOCK_PASSPHRASE
    expect(passphrase).toBeTruthy()
    const profile = process.env.E2E_UC_PROFILE
    expect(profile).toMatch(/^wdio-[a-z0-9-]*profile-key-recovery[a-z0-9-]*$/)
    const dataDir = path.join(
      homedir(),
      'Library/Application Support',
      `app.uniclipboard.desktop-${profile}`
    )
    const screenshots = path.resolve('e2e/artifacts', profile)
    fs.mkdirSync(screenshots, { recursive: true })
    await browser.tauri.switchWindow('main')
    if (process.env.E2E_RECOVERY_RESTART === '1') {
      await enterHistory(passphrase)
      expectHistoryReadable(profile, 'profile-recovery-original-history')
      expectHistoryReadable(profile, 'profile-recovery-new-history')
      expect(await $('#recovery-passphrase').isExisting()).toBe(false)
      const response = await daemonRequest(daemonConnection(profile), '/encryption/recovery')
      expect((await response.json()).data).toMatchObject({
        backgroundReady: true,
        restartRequired: false,
      })
      await settleFiniteAnimations()
      await browser.saveScreenshot(path.join(screenshots, 'restarted-history.png'))
      await browser.tauri.switchWindow('quick-panel')
      await $('input').waitForExist({ timeout: 30000 })
      await browser.tauri.switchWindow('main')
      return
    }
    const databases = fs
      .readdirSync(dataDir, { recursive: true })
      .filter(
        name =>
          name === 'uniclipboard.db' ||
          name.endsWith('/profile.sqlite') ||
          name.endsWith('/control.sqlite')
      )
    expect(databases.length).toBeGreaterThan(0)
    const protectedFiles = ['vault/profile-secrets-v1', ...databases]
    const before = protectedFiles.map(name => fs.readFileSync(path.join(dataDir, name)))
    const input = await $('#recovery-passphrase')
    await input.waitForDisplayed({ timeout: 60000 })
    expect(await $('body').getText()).toContain('剪贴板监听和设备同步暂不可用')
    expect(await $('[data-testid="history-search-anchor"]').isExisting()).toBe(false)
    await browser.saveScreenshot(path.join(screenshots, 'recovery.png'))
    await input.setValue(`${passphrase}-wrong`)
    await $('button[type="submit"]').click()
    await browser.waitUntil(async () => (await $('body').getText()).includes('口令错误'), {
      timeout: 30000,
    })
    await browser.saveScreenshot(path.join(screenshots, 'wrong-passphrase.png'))
    protectedFiles.forEach((name, index) => {
      expect(fs.readFileSync(path.join(dataDir, name)).equals(before[index])).toBe(true)
    })
    await input.setValue(passphrase)
    const failStartup = process.env.E2E_RECOVERY_FAIL_STARTUP === '1'
    const database = path.join(
      dataDir,
      databases.find(name => name === 'uniclipboard.db') ??
        databases.find(name => name.endsWith('/profile.sqlite'))
    )
    const backup = path.join(dataDir, 'startup-failure-database-backup')
    if (failStartup) {
      // An empty directory blocks SQLite opening; preserve and restore the isolated database.
      fs.renameSync(database, backup)
      fs.mkdirSync(database)
    }
    try {
      await $('button[type="submit"]').click()
      if (failStartup) {
        const restart = await $('button=重启后台服务')
        await restart.waitForDisplayed({ timeout: 60000 })
        await restart.waitForEnabled({ timeout: 30000 })
        expect(await $('body').getText()).toContain('后台服务启动失败，需要重启')
        expect(await $('button[type="submit"]').isExisting()).toBe(false)
        const failed = await daemonRequest(daemonConnection(profile), '/encryption/recovery')
        expect((await failed.json()).data).toMatchObject({
          state: 'failed',
          canSubmitPassphrase: false,
          restartRequired: true,
          backgroundReady: false,
        })
        await browser.saveScreenshot(path.join(screenshots, 'restart-required.png'))
      }
    } finally {
      if (failStartup) {
        fs.rmdirSync(database)
        fs.renameSync(backup, database)
      }
    }
    if (failStartup) {
      const previousPid = daemonConnection(profile).pid
      await $('button=重启后台服务').click()
      await browser.waitUntil(async () => daemonConnection(profile).pid !== previousPid, {
        timeout: 60000,
      })
      await enterHistory(passphrase)
    }
    await browser.waitUntil(
      async () => !(await input.isExisting()) || (await $('[role="alert"]').isDisplayed()),
      { timeout: 60000 }
    )
    if (await input.isExisting()) {
      await browser.saveScreenshot(path.join(screenshots, 'recovery-failed.png'))
      throw new Error(
        'Original passphrase recovery failed; see recovery-failed.png and daemon logs'
      )
    }
    await input.waitForExist({ reverse: true, timeout: 60000 })
    await enterHistory(passphrase)
    expectHistoryReadable(profile, 'profile-recovery-original-history')
    await settleFiniteAnimations()
    await browser.saveScreenshot(path.join(screenshots, 'recovered-history.png'))
    await browser.tauri.switchWindow('quick-panel')
    await $('input').waitForExist({ timeout: 30000 })
    await browser.tauri.switchWindow('main')
    const recoveredConnection = daemonConnection(profile)
    const recoveryResponse = await daemonRequest(recoveredConnection, '/encryption/recovery')
    expect((await recoveryResponse.json()).data).toMatchObject({
      state: failStartup ? 'not_required' : 'recovered',
      backgroundReady: true,
      restartRequired: false,
    })
  })
})
