import assert from 'node:assert/strict'
import { spawn, execFileSync } from 'node:child_process'
import { once } from 'node:events'
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import path from 'node:path'
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
const exited = once(native, 'close')
const lines = createInterface({ input: native.stdout })
const linesClosed = once(lines, 'close')
const timer = setTimeout(() => native.stdin.end(), 180_000)
let baseline
let cleanup
let connected
let failure
let dataDir
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
  const privateData = path.resolve(ready.dataDir)
  assert.equal(path.dirname(privateData).toLowerCase(), path.resolve(tmpdir()).toLowerCase())
  assert.ok(path.basename(privateData).startsWith('uc-gui-go-webview-probe-'))
  dataDir = privateData
  const endpoint = `http://127.0.0.1:${port}`
  for (let retry = 0; retry < 100; retry++) {
    try {
      connected = await chromium.connectOverCDP(endpoint, { timeout: 1000 })
      break
    } catch {
      if (native.exitCode !== null)
        throw new Error(`owned native probe exited ${native.exitCode} before CDP readiness`)
      await new Promise(resolve => setTimeout(resolve, 200))
    }
  }
  assert.ok(connected, 'the owned WebView2 CDP endpoint must be reachable')
  // CDP is published while the native controller is still being constructed.
  // Wait for its original target's navigation instead of quitting during Embed.
  let page
  for (let retry = 0; retry < 150; retry++) {
    page = connected
      .contexts()
      .flatMap(context => context.pages())
      .find(candidate => candidate.url().startsWith(ready.url))
    if (page) break
    await new Promise(resolve => setTimeout(resolve, 200))
  }
  assert.ok(page, 'the native control must finish navigating its original page')
  await page.getByRole('button', { name: 'isolated WebView2' }).waitFor({ timeout: 30_000 })
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
  connected = undefined
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
} catch (error) {
  failure = error
  await writeFile(
    'target/tester-army-native/failure.json',
    JSON.stringify(
      {
        status: 'probe-execution-failed',
        message: error.message,
        testerArmyAcceptance: false,
      },
      null,
      2
    ) + '\n'
  )
  throw error
} finally {
  await connected?.close().catch(() => undefined)
  clearTimeout(timer)
  native.stdin.end()
  const after = ms =>
    new Promise(resolve => {
      const timeout = setTimeout(() => resolve(null), ms)
      timeout.unref()
    })
  let outcome = await Promise.race([exited, after(15_000)])
  let forced = false
  let cleanupError
  if (outcome === null) {
    try {
      // Only signal the directly owned child after verifying its executable identity.
      const actual = execFileSync(
        'powershell.exe',
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `(Get-Process -Id ${native.pid} -ErrorAction Stop).Path`,
        ],
        { encoding: 'utf8', timeout: 3000 }
      ).trim()
      assert.equal(
        path.resolve(actual).toLowerCase(),
        path.resolve('target/webview-probe.exe').toLowerCase()
      )
      native.kill()
      forced = true
    } catch (error) {
      cleanupError = error.message
    }
    outcome = await Promise.race([exited, after(5000)])
  }
  if (outcome !== null) await linesClosed
  const code = outcome?.[0] ?? null
  let dataRemoved = false
  // WebView2 may release its profile after the host exits. Never delete an active profile.
  if (outcome !== null && dataDir) {
    for (let retry = 0; retry < 200; retry++) {
      try {
        await rm(dataDir, { recursive: true, force: true })
        dataRemoved = true
        break
      } catch (error) {
        cleanupError = error.message
        await new Promise(resolve => setTimeout(resolve, 100))
      }
    }
    if (dataRemoved) cleanupError = undefined
  }
  await writeFile(
    'target/tester-army-native/cleanup.json',
    JSON.stringify(
      {
        exitCode: code,
        forced,
        cleanupError,
        ...cleanup,
        dataRemoved,
      },
      null,
      2
    ) + '\n'
  )
  if (!failure) {
    assert.notEqual(outcome, null, 'owned native process did not stop within its cleanup budget')
    assert.equal(code, 0, 'owned native process must exit cleanly')
    assert.equal(cleanup?.windowClosed, true, 'the native control must close normally')
    assert.equal(dataRemoved, true, 'private WebView2 data must be removed')
  }
}
