import assert from 'node:assert/strict'
import { readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { test, outputFor } from './page.mjs'
import { selectedPlatform as platform } from './suites.mjs'
for (const language of ['zh-CN', 'en-US']) {
  for (const scheme of ['light', 'dark']) {
    test(`device-tasks ${scheme} ${language}`, async ({ app, page }) => {
      const base = app.baseUrl.replace(/\/$/, '')
      const output = await outputFor('device-tasks')
      const escape = text => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      assert.ok(['macos', 'windows', 'linux'].includes(platform))
      const results = []
      const {
        devices: d,
        history,
        unlock,
      } = JSON.parse(
        await readFile(new URL(`../../src/i18n/locales/${language}.json`, import.meta.url), 'utf8')
      )
      const errors = []
      page.on('pageerror', error => errors.push(error.message))
      const visit = async state =>
        page.goto(
          `${base}/?${new URLSearchParams({ scheme, language, state: state ?? 'enabled' })}`
        )
      const button = name => page.getByRole('button', { name, exact: true })
      const shot = async name => {
        assert.equal(await page.getByRole('dialog').count(), 1)
        const bounds = await page.getByRole('dialog').boundingBox()
        const viewport = page.viewportSize()
        assert.ok(
          bounds &&
            bounds.x >= 0 &&
            bounds.y >= 0 &&
            bounds.x + bounds.width <= viewport.width + 1 &&
            bounds.y + bounds.height <= viewport.height + 1
        )
        await page.screenshot({
          path: path.join(output, `${platform}-${language}-${scheme}-${name}.png`),
        })
        for (const action of await page.locator('section > footer button').all()) {
          const box = await action.boundingBox()
          assert.ok(
            box && box.y >= bounds.y && box.y + box.height <= bounds.y + bounds.height - 2,
            `${name}: task action clipped`
          )
        }
      }
      await visit()
      await button(d.kit.joinSpace).click()
      const code = page.getByLabel(d.switchSpace.labels.code, { exact: true })
      await code.waitFor()
      assert.equal(await page.locator('#switch-pass').count(), 0)
      await shot('join-code')
      await code.pressSequentially('012345')
      await button(d.tasks.continue).click()
      const pass = page.getByLabel(d.switchSpace.labels.newPassphrase, { exact: true })
      await pass.waitFor()
      await shot('join-confirm')
      await pass.pressSequentially('fixture-passphrase')
      assert.equal(await pass.inputValue(), 'fixture-passphrase')
      await button(d.tasks.back).click()
      await button(d.switchSpace.backToDevices).click()
      await button(d.kit.joinSpace).click()
      assert.equal(await code.inputValue(), '')
      await button(d.switchSpace.backToDevices).click()
      await button(d.kit.connectPhone).click()
      assert.equal(await page.getByRole('tab').count(), 0)
      await shot('phone-method')
      await page
        .getByRole('button', { name: new RegExp(`^${escape(d.connectMobile.regular)}`) })
        .click()
      const label = page.getByLabel(d.mobileSync.add.labelField.label, { exact: true })
      await label.waitFor()
      await shot('phone-name')
      await page.setViewportSize({ width: 640, height: 640 })
      await page.getByText(d.mobileSync.add.advanced.title, { exact: true }).click()
      await shot('phone-advanced-narrow')
      await label.fill('Test phone')
      await page.getByLabel(d.mobileSync.add.username.label, { exact: true }).fill('phoneUser')
      const password = page.getByLabel(d.mobileSync.add.password.label, { exact: true })
      await password.fill('fixture-password')
      await password.press('Enter')
      await page.getByLabel(d.mobileSync.credential.pair.qrAlt, { exact: true }).waitFor()
      await shot('phone-complete')
      await page.getByText(d.mobileSync.deviceDialog.manualEntry.title, { exact: true }).click()
      assert.equal(await page.getByText('fixture-password', { exact: true }).count(), 0)
      await button(unlock.passphraseModal.show).click()
      await page.getByText('fixture-password', { exact: true }).waitFor()
      await button(unlock.passphraseModal.hide).click()
      await button(history.composite.done).click()
      assert.equal(
        await page.getByLabel(d.mobileSync.credential.pair.qrAlt, { exact: true }).count(),
        0
      )
      assert.deepEqual(await page.evaluate(() => window.__taskRegistration), {
        label: 'Test phone',
        username: 'phoneUser',
        password: 'fixture-password',
      })
      await page.setViewportSize({ width: 1000, height: 800 })
      await visit('disabled')
      await button(d.kit.connectPhone).click()
      await page
        .getByRole('button', { name: new RegExp(`^${escape(d.connectMobile.regular)}`) })
        .click()
      await button(d.mobileSync.enableConfirm.confirm).waitFor()
      await shot('phone-enable')
      await button(d.mobileSync.enableConfirm.confirm).click()
      await label.waitFor()
      await visit('error')
      await button(d.kit.connectPhone).click()
      await page
        .getByRole('button', { name: new RegExp(`^${escape(d.connectMobile.regular)}`) })
        .click()
      await button(d.mobileSync.configure).click()
      await page.getByLabel(d.mobileSync.lanListener.port.label, { exact: true }).waitFor()
      await shot('phone-network')
      const port = page.getByLabel(d.mobileSync.lanListener.port.label, { exact: true })
      await port.fill('0')
      await button(d.tasks.save).click()
      await page.getByText(d.tasks.invalidPort, { exact: true }).waitFor()
      await port.fill('42721')
      await button(d.tasks.save).click()
      await page.getByText(d.mobileSync.lanListener.warning.body).waitFor()
      assert.equal(await page.getByRole('dialog').count(), 1)
      await button(d.tasks.back).click()
      assert.equal(await port.inputValue(), '42721')
      await visit()
      await button(d.kit.connectPhone).click()
      await page
        .getByRole('button', { name: new RegExp(`^${escape(d.tasks.directTitle)}`) })
        .click()
      await page.getByTestId('add-device-invitation-code').waitFor()
      assert.deepEqual(errors, [])
      results.push({
        platform,
        language,
        scheme,
        singleDialog: true,
        codeAndConfirmation: true,
        registration: true,
        enable: true,
        network: true,
      })
      await writeFile(
        path.join(output, `${language}-${scheme}.json`),
        JSON.stringify(results, null, 2)
      )
    })
  }
}
