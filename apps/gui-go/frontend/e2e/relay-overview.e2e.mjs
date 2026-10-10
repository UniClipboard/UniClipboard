import assert from 'node:assert/strict'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { build, loadConfigFromFile } from 'vite'

// The Vite config and the fixture entry resolve against the GUI package root.
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright')
const fixtureEntry = 'e2e/fixtures/settings.tsx'
const outDir = await mkdtemp(path.join(tmpdir(), 'relay-overview-e2e-'))
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
  const url = `${pathToFileURL(path.join(outDir, 'index.html')).href}?category=network`

  // Optional evidence: RELAY_OVERVIEW_ARTIFACT_DIR receives a screenshot and the DOM of the relay group per scenario.
  const artifactDir = process.env.RELAY_OVERVIEW_ARTIFACT_DIR
  if (artifactDir) await mkdir(artifactDir, { recursive: true })
  const capture = async name => {
    if (!artifactDir) return
    const group = page.locator('fieldset', { has: page.getByTestId('built-in-relay-list') })
    const target = (await group.count()) ? group : page.locator('body')
    await target.screenshot({ path: path.join(artifactDir, `${name}.png`) })
    await writeFile(path.join(artifactDir, `${name}.html`), await target.evaluate(n => n.outerHTML))
  }
  const entry = (source, regionId, relayUrl, inEffect, credentialConfigured = false) => ({
    source,
    regionId,
    url: relayUrl,
    credentialConfigured,
    inEffect,
  })
  const BUILT_IN = [
    ['na-east', 'https://use1-1.relay.n0.iroh.link./'],
    ['na-west', 'https://usw1-1.relay.n0.iroh.link./'],
    ['eu', 'https://euc1-1.relay.n0.iroh.link./'],
    ['asia-pacific', 'https://aps1-1.relay.n0.iroh.link./'],
  ]
  const builtIn = inEffect => BUILT_IN.map(([id, u]) => entry('builtIn', id, u, inEffect))
  // The harness scripts every overview request from Node so state survives reloads.
  let scripted = { response: null, failNext: 0 }
  const script = (response, failNext = 0) => {
    scripted = { response, failNext }
  }
  await page.exposeFunction('__relayOverviewScript', () => {
    if (scripted.failNext > 0) {
      scripted.failNext -= 1
      return { fail: true }
    }
    return { response: scripted.response ?? undefined }
  })
  const builtInOverviewFor = inEffect => ({
    savedMode: 'builtIn',
    appliedMode: 'builtIn',
    changePending: false,
    entries: builtIn(inEffect),
  })
  const rows = () => page.getByTestId('built-in-relay-row')
  const routing = () => page.getByTestId('relay-overview-routing')
  const applied = () => page.getByTestId('relay-overview-applied')

  // 1. Built-in routing, applied, no pending change: four localized rows, no controls.
  script(builtInOverviewFor(true))
  await page.goto(url)
  await page.getByRole('heading', { level: 1, name: '网络设置' }).waitFor()
  await rows().first().waitFor()
  assert.equal(await rows().count(), 4)
  assert.deepEqual(
    await rows().evaluateAll(nodes => nodes.map(n => n.getAttribute('data-region-id'))),
    ['na-east', 'na-west', 'eu', 'asia-pacific']
  )
  const list = page.getByTestId('built-in-relay-list')
  await list.getByText('北美东部', { exact: true }).waitFor()
  await list.getByText('亚太', { exact: true }).waitFor()
  await list.getByText('https://use1-1.relay.n0.iroh.link./', { exact: true }).waitFor()
  assert.equal(await list.getByText(/已配置到运行中的节点/).count(), 4)
  assert.equal(await list.locator('button, input, [role="switch"]').count(), 0)
  assert.equal(await routing().getAttribute('data-routing-mode'), 'builtIn')
  assert.equal(await applied().getAttribute('data-applied-mode'), 'builtIn')
  const pageText = await page.locator('body').innerText()
  assert.ok(!/已连接|connected/i.test(pageText.replace('并不表示已经连上某个中继', '')))

  await capture('01-built-in-applied')
  // 2. Reload with applied mode absent: node not started, nothing claimed as in effect.
  script({
    savedMode: 'builtIn',
    appliedMode: null,
    changePending: false,
    entries: builtIn(false),
  })
  await page.reload()
  await rows().first().waitFor()
  await applied().getByText('网络节点尚未启动').waitFor()
  assert.equal(await applied().getAttribute('data-applied-mode'), 'none')
  assert.equal(await page.locator('[data-in-effect="true"]').count(), 0)

  await capture('02-node-not-started')
  // 3. change_pending: the pending notice is shown and in_effect is not flipped locally.
  script({
    savedMode: 'builtIn',
    appliedMode: 'custom',
    changePending: true,
    entries: builtIn(false),
  })
  await page.reload()
  await rows().first().waitFor()
  await applied()
    .getByText(/下次重建/)
    .waitFor()
  assert.equal(await applied().getAttribute('data-change-pending'), 'true')
  assert.equal(await page.locator('[data-in-effect="true"]').count(), 0)

  await capture('03-change-pending')
  // 4. Custom list replaces built-in: rows dimmed and marked not used; custom rows stay editable.
  script({
    savedMode: 'custom',
    appliedMode: 'custom',
    changePending: false,
    entries: [
      ...builtIn(false),
      entry('custom', null, 'https://relay-one.example.com/', true, true),
    ],
  })
  await page.goto(`${url}&relayE2E=1`)
  await rows().first().waitFor()
  assert.equal(await routing().getAttribute('data-routing-mode'), 'custom')
  assert.equal(await list.getByText('未使用').count(), 4)
  assert.equal(await rows().count(), 4)
  await page.locator('input[value="https://relay-one.example.com/"]').waitFor()

  await capture('04-custom-replaces-built-in')
  // 5. Disabled (LAN-only) wins: listed, none in effect, dimmed.
  script({
    savedMode: 'disabled',
    appliedMode: 'disabled',
    changePending: false,
    entries: builtIn(false),
  })
  await page.goto(url)
  await rows().first().waitFor()
  assert.equal(await routing().getAttribute('data-routing-mode'), 'disabled')
  await routing()
    .getByText(/中继已关闭/)
    .waitFor()
  assert.equal(await page.locator('[data-in-effect="true"]').count(), 0)

  await capture('05-lan-only-disabled')
  // 6. Unknown future region id falls back to the URL.
  script({
    savedMode: 'builtIn',
    appliedMode: 'builtIn',
    changePending: false,
    entries: [entry('builtIn', 'sa-east', 'https://sae1-1.relay.example.net./', true)],
  })
  await page.goto(url)
  await rows().first().waitFor()
  assert.equal(
    await rows().first().locator('span').first().innerText(),
    'https://sae1-1.relay.example.net./'
  )

  await capture('06-unknown-region-url-fallback')
  // 7. Query failure shows a retryable error, never an empty list; retry recovers.
  script(
    { savedMode: 'builtIn', appliedMode: 'builtIn', changePending: false, entries: builtIn(true) },
    1
  )
  await page.goto(url)
  await page.getByText('无法加载内置中继列表。', { exact: true }).waitFor()
  assert.equal(await rows().count(), 0)
  await capture('06b-load-error-retry')
  await page.getByRole('button', { name: '重试' }).click()
  await rows().first().waitFor()
  assert.equal(await rows().count(), 4)

  await page.getByRole('button', { name: '重试' }).waitFor({ state: 'detached' })
  await capture('07-after-retry')
  assert.deepEqual(pageErrors, [])
  console.log(
    'PASS: built-in relay overview — localized rows, URL fallback, read-only, applied/not-started/change-pending, custom replacement, LAN-only, and retryable failure'
  )
} finally {
  await browser.close()
  await rm(outDir, { recursive: true, force: true })
}
