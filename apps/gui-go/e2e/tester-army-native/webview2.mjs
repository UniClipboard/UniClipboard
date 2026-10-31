import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { createServer } from 'node:net'
import { createInterface } from 'node:readline'
import { pathToFileURL } from 'node:url'

if (process.platform !== 'win32') throw new Error('Run only on a disposable Windows runner')
const require = createRequire(import.meta.url)
const playwright = await import(
  pathToFileURL(require.resolve('playwright-core', { paths: [require.resolve('@e2e-dev/web')] }))
)
const { chromium } = playwright.default || playwright
const listener = createServer()
await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve))
const port = listener.address().port
await new Promise(resolve => listener.close(resolve))
await mkdir('target/tester-army-native', { recursive: true })
const native = spawn('target/webview-probe.exe', [String(port)], {
  stdio: ['pipe', 'pipe', 'inherit'],
})
const exited = once(native, 'exit')
const lines = createInterface({ input: native.stdout })
const timer = setTimeout(() => native.stdin.end(), 90_000)
let baseline
let cleanup
try {
  const ready = await Promise.race([
    new Promise(resolve =>
      lines.on('line', line => {
        try {
          const value = JSON.parse(line)
          if (value.url) resolve(value)
          if (value.phase === 'cleanup') cleanup = value
        } catch {
          /* Wails diagnostic line. */
        }
      })
    ),
    exited.then(([code]) => {
      throw new Error(`owned native probe exited ${code}`)
    }),
  ])
  const endpoint = `http://127.0.0.1:${port}`
  let connected
  for (let retry = 0; retry < 100; retry++) {
    try {
      connected = await chromium.connectOverCDP(endpoint, { timeout: 1000 })
      break
    } catch {
      await new Promise(resolve => setTimeout(resolve, 200))
    }
  }
  assert.ok(connected, 'the owned WebView2 CDP endpoint must be reachable')
  const pages = connected.contexts().flatMap(context => context.pages())
  const page = pages.find(page => page.url().startsWith(ready.url))
  assert.ok(page, 'the native control must expose its original page')
  assert.equal(await page.getByRole('button').textContent(), 'isolated WebView2')
  const session = await page.context().newCDPSession(page)
  const targetId = (await session.send('Target.getTargetInfo')).targetInfo.targetId
  await session.detach()
  baseline = {
    originalNativeTarget: targetId,
    nativeReadback: true,
    isolatedData: ready.isolatedData,
  }
  await connected.close()
  const runner = spawn(
    process.execPath,
    ['node_modules/e2e/dist/cli/bin.js', 'run', '--config', 'e2e.native.config.ts'],
    {
      env: {
        ...process.env,
        E2E_TELEMETRY_DISABLED: '1',
        UC_NATIVE_URL: ready.url,
        UC_NATIVE_ENDPOINT: endpoint,
        UC_NATIVE_TARGET_ID: targetId,
      },
      stdio: 'inherit',
    }
  )
  const [code] = await once(runner, 'exit')
  const report = JSON.parse(
    await readFile('target/tester-army-native/framework/report.json', 'utf8')
  )
  const unsupported =
    /default context identity is unavailable|Browser context management is not supported|Target.getBrowserContexts.*wasn't found/.test(
      JSON.stringify(report)
    )
  const result = {
    ...baseline,
    frameworkExitCode: code,
    testerArmyAcceptance: code === 0,
    status:
      code === 0
        ? 'supported-original-native-target'
        : unsupported
          ? 'unsupported-context-contract'
          : 'unclassified-failure',
  }
  await writeFile('target/tester-army-native/webview2.json', JSON.stringify(result, null, 2) + '\n')
  assert.ok(code === 0 || unsupported, 'do not classify unrelated failures as unsupported')
} finally {
  clearTimeout(timer)
  native.stdin.end()
  const [code] = await exited
  lines.close()
  assert.equal(code, 0, 'owned native process must exit cleanly')
  assert.equal(cleanup?.dataRemoved, true, 'private WebView2 data must be removed')
  await writeFile(
    'target/tester-army-native/cleanup.json',
    JSON.stringify({ exitCode: code, ...cleanup }, null, 2) + '\n'
  )
}
