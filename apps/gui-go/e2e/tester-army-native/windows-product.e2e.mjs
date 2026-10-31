import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import { surfaceOf, test } from '@e2e-dev/web'
import { runScenarios } from '../../frontend/e2e/history-kit-browser.mjs'
import { runStartupScenarios } from '../windows-startup-scenarios.mjs'
import { nativeEngine } from './engine.mjs'

const output = 'target/tester-army-windows-product'
test('Windows product: original native startup, setup and empty history', async ({ app }) => {
  await mkdir(output, { recursive: true })
  await app.open('/')
  const page = surfaceOf(nativeEngine).page()
  const session = await page.context().newCDPSession(page)
  try {
    const { targetInfo } = await session.send('Target.getTargetInfo')
    await writeFile(
      `${output}/native-target.json`,
      JSON.stringify(
        { expected: process.env.UC_NATIVE_TARGET_ID, actual: targetInfo.targetId, url: page.url() },
        null,
        2
      ) + '\n'
    )
    assert.equal(
      targetInfo.targetId,
      process.env.UC_NATIVE_TARGET_ID,
      'drive the original product target'
    )
  } finally {
    await session.detach()
  }
  const evidence = []
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
  const waitFor = async (probe, label, timeout = 60_000) => {
    const end = Date.now() + timeout
    while (Date.now() < end) {
      const value = await probe()
      if (value) return value
      await delay(250)
    }
    throw new Error(`Timed out: ${label}`)
  }
  await mkdir(output, { recursive: true })
  try {
    await runStartupScenarios({
      waitFor,
      evaluate: expression => page.evaluate(expression),
      evidence,
      delay,
    })
    await runScenarios({
      daemon: { seeded: 0, profile: process.env.UC_NATIVE_PROFILE },
      output,
      platform: 'windows',
      cdp: true,
      browser: page.context().browser(),
      nativePage: page,
    })
    await app.screenshot('product-history')
  } finally {
    await writeFile(`${output}/startup-ready.json`, JSON.stringify(evidence, null, 2) + '\n')
  }
})
