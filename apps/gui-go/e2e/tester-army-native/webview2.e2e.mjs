import assert from 'node:assert/strict'
import { surfaceOf, test } from '@e2e-dev/web'
import { expect } from 'e2e'
import { nativeEngine } from './engine.mjs'

test('the original native WebView2 target receives an action', async ({ app, screen }) => {
  await app.open('/')
  const page = surfaceOf(nativeEngine).page()
  const session = await page.context().newCDPSession(page)
  try {
    const info = (await session.send('Target.getTargetInfo')).targetInfo
    assert.equal(
      info.targetId,
      process.env.UC_NATIVE_TARGET_ID,
      'must drive the native control itself'
    )
  } finally {
    await session.detach()
  }
  await screen.getByRole('button', { name: 'isolated WebView2' }).click()
  await expect(screen.getByRole('button', { name: 'native action confirmed' })).toBeVisible()
  await app.screenshot('native-webview2-action')
})
