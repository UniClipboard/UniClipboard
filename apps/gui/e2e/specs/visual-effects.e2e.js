import assert from 'node:assert/strict'
import { browser } from '@wdio/globals'

async function invoke(command, args = {}) {
  return browser.executeAsync(
    (name, input, done) => {
      window.__TAURI_INTERNALS__
        .invoke(name, input)
        .then(done)
        .catch(error => done({ error: String(error) }))
    },
    command,
    args
  )
}

describe('device-local visual effects', () => {
  it('applies native preferences to the mounted window and preserves the GUI session', async () => {
    const before = await invoke('set_visual_effects_mode', { mode: 'auto' })
    assert.equal(before.mode, 'auto')
    const allowed = await invoke('report_visual_effects_environment', {
      sessionId: before.sessionId,
      systemMotion: 'allow',
    })
    if (process.env.E2E_EXPECT_AUTO) {
      assert.equal(allowed.autoForSession, process.env.E2E_EXPECT_AUTO)
    }
    console.log('Native automatic visual effects:', {
      result: allowed.autoForSession,
      reason: allowed.reason,
      lowEffects: allowed.lowEffects,
    })
    const manual = await invoke('set_visual_effects_mode', { mode: 'effects' })
    assert.equal(manual.mode, 'effects')
    assert.equal(manual.lowEffects, false)
    assert.equal(manual.persistence, 'saved')
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.documentElement.dataset.ucLowEffects)) === 'false'
    )
    const origin = await browser.execute(() => performance.timeOrigin)
    await browser.refresh()
    await browser.waitUntil(async () =>
      browser.execute(
        previous =>
          performance.timeOrigin !== previous &&
          document.readyState === 'complete' &&
          document.documentElement.dataset.ucLowEffects === 'false',
        origin
      )
    )
    const reopened = await invoke('get_visual_effects')
    assert.equal(reopened.sessionId, before.sessionId)
    assert.equal(reopened.mode, 'effects')
    const auto = await invoke('set_visual_effects_mode', { mode: 'auto' })
    assert.equal(auto.autoForSession, before.autoForSession)
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => document.documentElement.dataset.ucLowEffects)) ===
        String(auto.lowEffects)
    )
  })
})
