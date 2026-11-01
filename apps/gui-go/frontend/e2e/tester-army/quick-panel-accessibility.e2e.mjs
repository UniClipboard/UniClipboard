import assert from 'node:assert/strict'
import { writeFile } from 'node:fs/promises'
import { test, outputFor } from './page.mjs'
import { selectedPlatform } from './suites.mjs'

// Failure cases: a disabled panel hides the entry; a stale check wins after focus;
// denied or failed checks look granted; an opener failure has no guidance; other OSes show macOS permissions.
for (const theme of ['light', 'dark']) {
  test(`quick panel accessibility ${theme}`, async ({ app, page }) => {
    const output = await outputFor('quick-panel-accessibility')
    await page.addInitScript(platform => {
      Object.defineProperty(navigator, 'userAgentData', {
        value: {
          platform: platform === 'macos' ? 'macOS' : platform === 'windows' ? 'Windows' : 'Linux',
        },
      })
      Object.defineProperty(navigator, 'platform', {
        value:
          platform === 'macos' ? 'MacIntel' : platform === 'windows' ? 'Win32' : 'Linux x86_64',
      })
      Object.defineProperty(navigator, 'userAgent', {
        value: platform === 'macos' ? 'Macintosh' : platform === 'windows' ? 'Windows' : 'Linux',
      })
    }, selectedPlatform)
    await page.setViewportSize({ width: 1100, height: 800 })
    await page.goto(app.baseUrl)
    await page.evaluate(
      theme => document.documentElement.classList.toggle('dark', theme === 'dark'),
      theme
    )
    assert.equal(
      await page.evaluate(() => navigator.userAgentData.platform),
      selectedPlatform === 'macos' ? 'macOS' : selectedPlatform === 'windows' ? 'Windows' : 'Linux'
    )
    await page.getByRole('button', { name: '快捷面板', exact: true }).click()
    const permission = page.getByText('辅助功能权限', { exact: true })
    const calls = () => page.evaluate(() => window.__ucNativeCalls)
    const checks = []
    const toggle = page.getByRole('switch', { name: '启用快捷面板' })
    await toggle.evaluate(element => element.setAttribute('data-e2e-panel-toggle', 'true'))
    const enabled = async value => {
      await page.waitForFunction(value => {
        const control = document.querySelector('[data-e2e-panel-toggle]')
        return (
          control &&
          ('checked' in control
            ? control.checked === value
            : control.getAttribute('aria-checked') === String(value))
        )
      }, value)
    }
    for (const value of [true, false]) {
      await page.evaluate(() => {
        window.__settingsFixtureNative.quickPanelSaveShouldFail = true
      })
      await toggle.click()
      await enabled(value)
      await page.evaluate(() => {
        window.__settingsFixtureNative.quickPanelSaveShouldFail = false
      })
      await toggle.click()
      await enabled(!value)
    }
    assert.equal(
      await page.getByText(/建议重启应用以彻底释放|Restart the app to fully release/).count(),
      0
    )
    assert.equal(await page.evaluate(() => window.__settingsFixtureNative.restartCalls), 0)
    checks.push('enable and disable save failures roll back; successful toggles need no restart')
    if (selectedPlatform !== 'macos') {
      assert.equal(await permission.count(), 0)
      assert.equal(
        (await calls()).some(call => call.command === 'GetQuickPanelAccessibilityPermission'),
        false
      )
      checks.push('macOS permission hidden and not queried')
    } else {
      await permission.waitFor()
      await page.getByText('未授权', { exact: true }).waitFor()
      const opener = page.getByRole('button', {
        name: '打开系统设置',
        exact: true,
      })
      // Disable the panel; authorization must remain available.
      await page.getByRole('switch', { name: '启用快捷面板' }).click()
      assert.equal(
        await page.getByRole('switch', { name: '启用快捷面板' }).getAttribute('aria-checked'),
        'false'
      )
      await opener.click()
      assert.deepEqual(await page.evaluate(() => window.__settingsFixtureNative.openedUrls), [
        'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
      ])
      checks.push('disabled panel still exposes working system-settings action')
      await page.screenshot({ path: `${output}/${theme}-denied.png` })
      const refreshBeforeGrant = (await calls()).filter(
        call => call.command === 'RefreshQuickPanelTriggers'
      ).length
      await page.evaluate(() => {
        window.__settingsFixtureNative.accessibilityGranted = true
        window.dispatchEvent(new Event('focus'))
      })
      await page.getByText('已授权', { exact: true }).waitFor()
      assert.equal(await opener.count(), 0)
      await page.waitForFunction(
        before =>
          window.__ucNativeCalls.filter(call => call.command === 'RefreshQuickPanelTriggers')
            .length > before,
        refreshBeforeGrant
      )
      checks.push('grant refreshed on return')
      await page.screenshot({ path: `${output}/${theme}-granted.png` })
      assert.ok(
        (await calls()).some(call => call.command === 'RefreshQuickPanelTriggers' && call.handled)
      )
      // Hold one response while permission changes and a burst of focus/visibility arrives.
      const before = (await calls()).filter(
        call => call.command === 'GetQuickPanelAccessibilityPermission'
      ).length
      await page.evaluate(() => {
        window.__settingsFixtureNative.holdAccessibilityCheck = true
        window.dispatchEvent(new Event('focus'))
      })
      await page.waitForFunction(() =>
        Boolean(window.__settingsFixtureNative.releaseAccessibilityCheck)
      )
      await page.evaluate(() => {
        window.__settingsFixtureNative.accessibilityGranted = false
        for (let i = 0; i < 5; i++) {
          window.dispatchEvent(new Event('focus'))
          document.dispatchEvent(new Event('visibilitychange'))
        }
        window.__settingsFixtureNative.holdAccessibilityCheck = false
        window.__settingsFixtureNative.releaseAccessibilityCheck()
      })
      await page.getByText('未授权', { exact: true }).waitFor()
      assert.equal(
        (await calls()).filter(call => call.command === 'GetQuickPanelAccessibilityPermission')
          .length - before,
        2
      )
      checks.push(
        'concurrent checks coalesce and trailing check observes revocation; helper refresh requested'
      )

      await page.screenshot({ path: `${output}/${theme}-revoked.png` })
      await page.evaluate(() => {
        window.__settingsFixtureNative.accessibilityShouldFail = true
        window.dispatchEvent(new Event('focus'))
      })
      await page.getByText('无法检查权限', { exact: true }).waitFor()
      assert.equal(await page.getByText('未授权', { exact: true }).count(), 0)
      await page.evaluate(() => {
        window.__settingsFixtureNative.accessibilityShouldFail = false
        window.__settingsFixtureNative.accessibilityGranted = false
      })
      await page.getByRole('button', { name: '重试', exact: true }).click()
      await page.getByText('未授权', { exact: true }).waitFor()
      checks.push('failed check is distinct and retry recovers')
      await page.evaluate(() => {
        window.__settingsFixtureNative.openSettingsShouldFail = true
      })
      await opener.click()
      await page
        .getByText('无法打开辅助功能设置，请手动前往「系统设置 > 隐私与安全性 > 辅助功能」。', {
          exact: true,
        })
        .waitFor()
      checks.push('opener failure shows manual instructions')
    }
    assert.equal(
      await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
      true
    )
    await page.screenshot({ path: `${output}/${theme}-final.png` })
    await writeFile(
      `${output}/${theme}-assertions.json`,
      JSON.stringify(
        {
          platform: selectedPlatform,
          theme,
          checks,
          scope:
            'real settings UI with scripted host responses; system authorization and typing not exercised',
        },
        null,
        2
      )
    )
  })
}
