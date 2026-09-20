import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'
import { build, loadConfigFromFile } from 'vite'

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const fixtureEntry = 'e2e/fixtures/settings.tsx'
const outDir = await mkdtemp(path.join(tmpdir(), 'custom-relays-e2e-'))
const loaded = await loadConfigFromFile({ command: 'build', mode: 'test' })

await build({
  ...loaded.config,
  configFile: false,
  logLevel: 'warn',
  build: {
    ...loaded.config.build,
    outDir,
    manifest: true,
    emptyOutDir: false,
    rollupOptions: { input: fixtureEntry },
  },
})
const manifest = JSON.parse(await readFile(path.join(outDir, '.vite/manifest.json'), 'utf8'))
const entry = manifest[fixtureEntry]
const assetUrl = file => pathToFileURL(path.join(outDir, file)).href
await writeFile(
  path.join(outDir, 'index.html'),
  `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">${(entry.css || []).map(file => `<link rel="stylesheet" href="${assetUrl(file)}">`).join('')}</head><body><div id="root"></div><script type="module" src="${assetUrl(entry.file)}"></script></body></html>`
)

const browser = await chromium.launch({
  headless: true,
  channel: process.env.PLAYWRIGHT_CHANNEL,
  executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH,
  args: ['--allow-file-access-from-files'],
})

try {
  const page = await browser.newPage({
    viewport: { width: 1100, height: 900 },
    locale: 'zh-CN',
    reducedMotion: 'reduce',
  })
  page.setDefaultTimeout(15_000)
  const pageErrors = []
  page.on('pageerror', error => pageErrors.push(error.message))

  await page.goto(
    `${pathToFileURL(path.join(outDir, 'index.html')).href}?category=network&relayE2E=1`
  )
  await page.getByRole('heading', { level: 1, name: '网络设置' }).waitFor()
  await page.locator('input[value="https://relay-one.example.com/"]').waitFor()
  assert.equal(await page.getByText('访问令牌已保存', { exact: true }).count(), 1)
  assert.equal(await page.getByRole('textbox', { name: /自定义中继节点/ }).count(), 2)

  await page.evaluate(() => {
    window.__customRelayFixture.injectServerRelay({
      url: 'https://remote-change.example.com/',
      credentialConfigured: false,
    })
  })

  const firstRelay = page.getByRole('textbox', { name: '自定义中继节点 1' })
  await firstRelay.fill('https://relay-one-renamed.example.com')
  const firstEditor = firstRelay.locator('xpath=ancestor::section')
  await firstEditor.getByRole('button', { name: '测试可用性' }).click()
  await firstEditor.getByText(/握手成功/).waitFor()
  await firstEditor.getByRole('button', { name: '保存中继节点' }).click()

  await page.locator('input[value="https://relay-one-renamed.example.com/"]').waitFor()
  await page.locator('input[value="https://remote-change.example.com/"]').waitFor()
  assert.equal(await page.getByRole('textbox', { name: /自定义中继节点/ }).count(), 3)
  const editMutation = await page.evaluate(() => window.__customRelayFixture.getMutations()[0])
  assert.deepEqual(editMutation, {
    action: 'edit',
    previousUrl: 'https://relay-one.example.com/',
    url: 'https://relay-one-renamed.example.com',
    credential: { action: 'keep' },
  })
  assert.equal(await page.getByText('访问令牌已保存', { exact: true }).count(), 1)

  await page.evaluate(() => window.__customRelayFixture.setRestartFailure(true))
  await page.getByRole('button', { name: '立即重启' }).click()
  await page.getByText('后台服务重启失败，请重试或手动重启应用。').waitFor()
  assert.equal(await page.getByRole('textbox', { name: /自定义中继节点/ }).count(), 3)
  await page.locator('input[value="https://remote-change.example.com/"]').waitFor()

  const secondRelay = page.getByRole('textbox', { name: '自定义中继节点 2' })
  await secondRelay.fill('https://relay-one-renamed.example.com')
  const secondEditor = secondRelay.locator('xpath=ancestor::section')
  await secondEditor.getByRole('button', { name: '测试可用性' }).click()
  await secondEditor.getByText(/握手成功/).waitFor()
  await secondEditor.getByRole('button', { name: '保存中继节点' }).click()
  await page
    .getByText(/中继 URL 已重复/)
    .first()
    .waitFor()

  await page.getByRole('button', { name: '添加中继节点' }).click()
  const invalidRelay = page.getByRole('textbox', { name: '自定义中继节点 4' })
  await invalidRelay.fill('ftp://invalid.example.com')
  const invalidEditor = invalidRelay.locator('xpath=ancestor::section')
  await invalidEditor.getByRole('button', { name: '测试可用性' }).click()
  await invalidEditor.getByText(/握手成功/).waitFor()
  await invalidEditor.getByRole('button', { name: '保存中继节点' }).click()
  await page
    .getByText(/无效的中继 URL/)
    .first()
    .waitFor()
  await invalidEditor.getByRole('button', { name: '移除中继节点 4' }).click()

  await page.evaluate(() => {
    window.__customRelayFixture.removeServerRelay('https://relay-two.example.com/')
  })
  await secondEditor.getByRole('button', { name: '移除中继节点 2' }).click()
  await page
    .getByText(/此中继已不存在/)
    .first()
    .waitFor()
  await page.locator('input[value="https://remote-change.example.com/"]').waitFor()
  assert.equal(await page.getByRole('textbox', { name: /自定义中继节点/ }).count(), 2)

  await page.getByRole('button', { name: '添加中继节点' }).click()
  const addedRelay = page.getByRole('textbox', { name: '自定义中继节点 3' })
  await addedRelay.fill('https://added.example.com')
  const addedEditor = addedRelay.locator('xpath=ancestor::section')
  await addedEditor.getByRole('textbox', { name: '中继访问令牌 3' }).fill('e2e-placeholder-token')
  await addedEditor.getByRole('button', { name: '测试可用性' }).click()
  await addedEditor.getByText(/握手成功/).waitFor()
  await addedEditor.getByRole('button', { name: '保存中继节点' }).click()
  const savedAddedRelay = page.locator('input[value="https://added.example.com/"]')
  await savedAddedRelay.waitFor()
  const savedAddedEditor = savedAddedRelay.locator('xpath=ancestor::section')
  await savedAddedEditor.getByText('访问令牌已保存', { exact: true }).waitFor()
  assert.equal(
    await savedAddedEditor.getByRole('textbox', { name: '中继访问令牌 3' }).inputValue(),
    ''
  )
  await savedAddedEditor.getByRole('button', { name: '移除中继节点 3' }).click()
  await savedAddedRelay.waitFor({ state: 'detached' })

  await page.evaluate(() => window.__customRelayFixture.showLoadError())
  await page.getByText('无法加载自定义中继列表。', { exact: true }).waitFor()
  await page.getByRole('button', { name: '重试' }).click()
  await page.locator('input[value="https://relay-one-renamed.example.com/"]').waitFor()

  assert.deepEqual(pageErrors, [])
  console.log(
    'PASS: authoritative list replacement, stable-address edit/delete, token migration/status, translated rejections, retry, and independent restart failure'
  )
} finally {
  await browser.close()
  await rm(outDir, { recursive: true, force: true })
}
