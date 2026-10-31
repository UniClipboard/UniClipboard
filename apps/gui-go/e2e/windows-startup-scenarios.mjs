import assert from 'node:assert/strict'

// Shared by the original CDP launcher and the TesterArmy native product suite.
export async function runStartupScenarios({ waitFor, evaluate, evidence, delay }) {
  const status = await waitFor(
    () => evaluate(`import('/src/lib/ipc.ts').then(m => m.commands.getDaemonStartupStatus())`),
    'startup status'
  )
  assert.equal(status.service_ready, true)
  evidence.push({
    step: 'native-startup-status',
    serviceReady: status.service_ready,
    state: status.progress.state,
  })
  await waitFor(
    () => evaluate(`!!document.querySelector('[data-testid="setup-entry-create"]')`),
    'setup screen leaves startup'
  )
  evidence.push({ step: 'setup-screen', ok: true })
  await evaluate(`document.querySelector('[data-testid="setup-entry-create"]').click()`)
  await waitFor(() => evaluate(`!!document.querySelector('#device-name')`), 'setup form')
  await evaluate(`(() => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    for (const [selector, value] of [['#device-name','synthetic-startup-device'],['#pass1','synthetic-startup-passphrase'],['#pass2','synthetic-startup-passphrase']]) {
      const input = document.querySelector(selector); setter.call(input,value); input.dispatchEvent(new Event('input',{bubbles:true}));
    }
  })()`)
  await delay(300)
  await evaluate(`document.querySelector('[data-testid="setup-initialize-submit"]').click()`)
  await waitFor(
    () =>
      evaluate(
        `!!document.querySelector('[data-testid="setup-complete-later"]') || location.pathname.startsWith('/history')`
      ),
    'setup completed'
  )
  await evaluate(`document.querySelector('[data-testid="setup-complete-later"]')?.click()`)
  await waitFor(
    () =>
      evaluate(
        `location.pathname.startsWith('/history') && !!document.querySelector('a[href="/settings"]')`
      ),
    'authenticated history screen'
  )
  evidence.push({ step: 'authenticated-history-screen', ok: true })
}
