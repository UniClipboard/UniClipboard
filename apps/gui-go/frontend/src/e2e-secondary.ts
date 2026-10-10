import { Call } from '@wailsio/runtime'
// E2E-only reporter for the updater and quick-panel windows. Bundled only
// when VITE_GUI_GO_E2E=1; the main window uses e2e-driver.ts.
import { openUrl } from '@/host/opener'

// The page's own external-link call (the one the shared frontend uses) so a scenario can drive the real URL-open chain
// with `panel-js`; the promise outcome goes to the scenario's loopback listener.
;(window as unknown as { __ucE2eOpenUrl: (url: string, report: string) => void }).__ucE2eOpenUrl = (
  url,
  report
) => {
  openUrl(url).then(
    () => void fetch(`${report}?v=ok`, { mode: 'no-cors' }),
    err => void fetch(`${report}?v=${encodeURIComponent(String(err))}`, { mode: 'no-cors' })
  )
}

export async function reportMounted(window: string, step: string, expectText?: string) {
  const deadline = Date.now() + 30000
  const root = document.getElementById('root')!
  while (Date.now() < deadline) {
    const text = document.body.innerText
    if (root.children.length > 0 && (!expectText || text.includes(expectText))) break
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  const text = document.body.innerText
  const ok = root.children.length > 0 && (!expectText || text.includes(expectText))
  await Call.ByName('main.EvidenceService.Record', {
    window,
    step,
    ok,
    detail: { path: location.pathname + location.search, text: text.slice(0, 160) },
  })
}

// The quick panel must follow the shared content lock: report what it shows once it is made visible.
export function reportPanelOnShow() {
  void import('@wailsio/runtime').then(({ Events }) =>
    Events.On('quick-panel://prepare-show', async () => {
      await new Promise(resolve => setTimeout(resolve, 1500))
      const text = document.body.innerText
      await Call.ByName('main.EvidenceService.Record', {
        window: 'quick-panel',
        step: 'quick-panel-shown-state',
        ok: !text.includes('解锁后才能查看'),
        detail: { text: text.slice(0, 160) },
      })
    })
  )
}

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))
const report = (step: string, ok: boolean, detail?: unknown) =>
  Call.ByName('main.EvidenceService.Record', { window: 'updater', step, ok, detail })

async function waitUntil(label: string, probe: () => boolean, ms = 60000) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (probe()) return
    await sleep(100)
  }
  throw new Error(`timeout: ${label}`)
}

// The primary action is the last button of the window (download, then install).
const primaryButton = () => [...document.querySelectorAll('button')].at(-1) as HTMLButtonElement

/** Updater window entry for E2E: dev preview check, or the real signed-update flow. */
export async function driveUpdater() {
  const phase = (await Call.ByName('main.EvidenceService.Phase')) as string
  if (!phase.startsWith('update')) {
    await reportMounted('updater', 'updater-mounted', '0.99.0')
    return
  }
  try {
    const { Events } = await import('@wailsio/runtime')
    let outcome: { event: string; error?: string } | null = null
    Events.On('update-download-progress', e => {
      const data = e.data as { event: string; data?: { error?: string } }
      if (data.event === 'Finished' || data.event === 'Failed')
        outcome = { event: data.event, error: data.data?.error }
    })
    await waitUntil('release shown', () => document.body.innerText.includes('99.0.0'))
    await report('updater-shows-release', true, { text: document.body.innerText.slice(0, 200) })
    primaryButton().click() // download
    await waitUntil('download outcome', () => outcome !== null, 120000)
    const result = outcome as unknown as { event: string; error?: string }
    if (phase === 'update-bad') {
      await report('update-download-rejected', result.event === 'Failed', { error: result.error })
      await Call.ByName('main.EvidenceService.Control', 'exit')
      return
    }
    await report('update-download-verified', result.event === 'Finished')
    await waitUntil('install button enabled', () => !primaryButton().disabled)
    await sleep(500)
    await report('update-install-clicked', true)
    primaryButton().click() // install and restart
  } catch (error) {
    await report('update-driver-error', false, String(error))
  }
}
