// Failure modes: the Detailed capture switch and the network window drift apart, a second network switch
// appears, the export during a capture lacks network records, the window does not close at expiry, or the
// offline export reports a complete state while the daemon is gone.
import { getDiagnosticCaptureStatus, getNetworkLogStatus } from '@/api/daemon/diagnostics'
import i18n from '@/i18n'

type RecordStep = (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
type Helpers = {
  record: RecordStep
  control: (action: string) => Promise<unknown>
  waitFor: <T>(label: string, probe: () => T | null | undefined | false, ms?: number) => Promise<T>
  sleep: (ms: number) => Promise<unknown>
  reachMainLayout: () => Promise<void>
  navControl: (href: string) => HTMLElement | null
}

const t = (key: string, options?: Record<string, unknown>) => i18n.t(key, options) as string

const captureSwitch = () =>
  Array.from(document.querySelectorAll<HTMLElement>('[role="switch"]')).find(
    node =>
      node.getAttribute('aria-label') === t('settings.sections.general.logs.capture.label') ||
      node
        .closest('[data-setting-row]')
        ?.textContent?.includes(t('settings.sections.general.logs.capture.label'))
  ) ?? null

const buttonWithText = (text: string) =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(
    b => b.textContent?.trim() === text
  ) ?? null

// The row text around the capture switch: its label plus the description beneath it.
function captureRowText() {
  const sw = captureSwitch()
  let node: HTMLElement | null = sw
  for (let i = 0; i < 6 && node; i++) {
    node = node.parentElement
    if (node?.textContent?.includes(t('settings.sections.general.logs.capture.label'))) {
      if (
        (node.textContent?.length ?? 0) >
        t('settings.sections.general.logs.capture.label').length + 10
      )
        return node.textContent!.trim()
    }
  }
  return ''
}

async function snapshot() {
  const [business, network] = await Promise.all([
    getDiagnosticCaptureStatus(),
    getNetworkLogStatus(),
  ])
  return {
    at: new Date().toISOString(),
    business: {
      mode: business.capture.mode,
      captureId: business.capture.captureId ?? null,
      remainingMs: business.capture.remainingMs,
      runId: business.runId,
      sourceCommit: business.sourceCommit,
    },
    network: {
      availability: network.availability,
      mode: network.mode ?? null,
      state: network.state ?? null,
      captureId: network.captureId ?? null,
      detailedRemainingMs: network.detailedRemainingMs ?? null,
      linkFailure: network.captureLinkFailure ?? null,
      runId: network.runId ?? null,
      counters: network.counters ?? null,
    },
  }
}

export async function runDiagnosticsAcceptance(phase: string, h: Helpers) {
  const { record, control, waitFor, sleep } = h
  const marker = phase.split(':')[2] ?? ''
  const scenario = phase.split(':')[1]
  await h.reachMainLayout()
  await waitFor('settings nav', () => h.navControl('/settings'))
  h.navControl('/settings')!.click()
  await waitFor('capture switch', () => {
    const general = buttonWithText(t('settings.categories.general'))
    if (general && !captureSwitch()) general.click()
    return captureSwitch()
  })
  // Scroll the logs group into view so the native screenshot shows it.
  captureSwitch()!.scrollIntoView({ block: 'center' })
  const switches = Array.from(document.querySelectorAll<HTMLElement>('[role="switch"]')).map(
    node => node.getAttribute('aria-label') ?? node.closest('div')?.textContent?.slice(0, 40) ?? ''
  )
  const networkSwitches = switches.filter(label => /network|connection|网络|连接/i.test(label))
  // A skin may render the switch as a checkbox input (state in `checked`) or as an ARIA switch (`aria-checked`).
  const isOn = () => {
    const node = captureSwitch()!
    return (
      node.getAttribute('aria-checked') === 'true' ||
      (node as HTMLInputElement).checked === true ||
      node.getAttribute('data-state') === 'checked'
    )
  }
  const settleUi = async () => {
    await waitFor('capture switch enabled', () => !captureSwitch()!.hasAttribute('disabled'))
  }
  const description = () => captureRowText()
  await settleUi()
  await record('diag-settings-open', true, {
    description: description(),
    expectedConnectionDetails: t('settings.sections.general.logs.capture.standard'),
    descriptionMentionsConnection: description().includes(
      t('settings.sections.general.logs.capture.standard')
    ),
    switchLabels: switches,
    networkSwitchCount: networkSwitches.length,
    initialSnapshot: await snapshot(),
  })

  if (scenario === 'capture') {
    // 2. start through the one switch and prove the two windows are linked.
    captureSwitch()!.click()
    await waitFor('switch on', () => isOn())
    let linked: Awaited<ReturnType<typeof snapshot>> | null = null
    for (let i = 0; i < 60 && !linked; i++) {
      const s = await snapshot()
      if (s.business.mode === 'detailed' && s.network.mode === 'detailed') linked = s
      else await sleep(500)
    }
    await record('diag-capture-started', !!linked, { snapshot: linked, ui: description() })

    // 3. the orchestrator sends the synthetic text from the other peer; wait for it in the daemon history.
    const { getClipboardEntries } = await import('@/api/daemon/clipboard')
    let received: unknown = null
    for (let i = 0; i < 240 && !received; i++) {
      const entries = (await getClipboardEntries(20, 0)).entries ?? []
      received = entries.find(entry => JSON.stringify(entry).includes(marker))
        ? { at: new Date().toISOString(), count: entries.length }
        : null
      if (!received) await sleep(1000)
    }
    await record('diag-marker-received', !!received, received)

    // 4. export through the real button while the capture is open.
    const exportButton = buttonWithText(t('settings.sections.general.logs.export.button'))!
    exportButton.scrollIntoView({ block: 'center' })
    exportButton.click()
    await waitFor(
      'export result',
      () => buttonWithText(t('settings.sections.general.logs.export.copyPath')),
      120000
    )
    const pathNode = buttonWithText(t('settings.sections.general.logs.export.copyPath'))!
      .previousElementSibling as HTMLElement | null
    await record('diag-export-during-capture', true, {
      path: pathNode?.textContent ?? null,
      notices: Array.from(document.querySelectorAll('p.text-right')).map(p => p.textContent),
      snapshot: await snapshot(),
    })

    // 5a. stop and start again through the switch; both sides must follow each time.
    captureSwitch()!.click()
    await waitFor('switch off', () => !isOn())
    let stopped: Awaited<ReturnType<typeof snapshot>> | null = null
    for (let i = 0; i < 60 && !stopped; i++) {
      const s = await snapshot()
      if (s.business.mode === 'standard' && s.network.mode !== 'detailed') stopped = s
      else await sleep(500)
    }
    await settleUi()
    await record('diag-capture-stopped', !!stopped, { snapshot: stopped, ui: description() })
    captureSwitch()!.click()
    await waitFor('switch on again', () => isOn())
    let restarted: Awaited<ReturnType<typeof snapshot>> | null = null
    for (let i = 0; i < 60 && !restarted; i++) {
      const s = await snapshot()
      if (s.business.mode === 'detailed' && s.network.mode === 'detailed') restarted = s
      else await sleep(500)
    }
    await record('diag-capture-restarted', !!restarted, { snapshot: restarted, ui: description() })

    if (phase.split(':')[3] === 'nowait') {
      // Orchestrator-only shortcut for the offline re-run; the full expiry is proven by another run.
      await record('diag-driver-complete', true)
      return control('exit')
    }
    // 5b. the full 600 s expiry; state is read from the daemon, not from the countdown.
    const startedAt = Date.now()
    const samples: unknown[] = []
    let expired: Awaited<ReturnType<typeof snapshot>> | null = null
    while (Date.now() - startedAt < 720_000 && !expired) {
      await sleep(5_000)
      const s = await snapshot()
      if (samples.length === 0 || Date.now() - startedAt > 590_000 || samples.length % 12 === 0)
        samples.push(s)
      if (s.business.mode === 'standard') expired = s
    }
    const elapsedMs = Date.now() - startedAt
    // Let one more poll pass so the daemon reconciles the network side.
    await sleep(6_000)
    const after = await snapshot()
    await sleep(6_000)
    await record(
      'diag-capture-expired',
      !!expired && elapsedMs >= 590_000 && after.network.mode !== 'detailed',
      { elapsedMs, firstStandard: expired, after, samples, ui: description(), switchOn: isOn() }
    )
    await record('diag-driver-complete', true)
    return control('exit')
  }

  if (scenario === 'reopen') {
    await record('diag-reopen-state', true, { snapshot: await snapshot(), ui: description() })
    await sleep(2_000)
    await record('diag-driver-complete', true)
    return control('exit')
  }

  if (scenario === 'offline') {
    const before = await snapshot()
    await record('diag-export-online', true, { snapshot: before })
    const exportButton = buttonWithText(t('settings.sections.general.logs.export.button'))!
    exportButton.click()
    await waitFor(
      'online export result',
      () => buttonWithText(t('settings.sections.general.logs.export.copyPath')),
      120000
    )
    await record('diag-export-after-reopen', true, {
      path:
        (
          buttonWithText(t('settings.sections.general.logs.export.copyPath'))!
            .previousElementSibling as HTMLElement | null
        )?.textContent ?? null,
      notices: Array.from(document.querySelectorAll('p.text-right')).map(p => p.textContent),
    })
    // The orchestrator now stops the task-owned daemon; wait until the daemon is unreachable.
    await record('diag-ready-for-offline', true)
    let down = false
    for (let i = 0; i < 180 && !down; i++) {
      down = await getDiagnosticCaptureStatus().then(
        () => false,
        () => true
      )
      if (!down) await sleep(2_000)
    }
    await record('diag-daemon-unreachable', down)
    // The stop request returns before the daemon process has exited; let it finish first.
    for (const end = Date.now() + 25_000; Date.now() < end;) await sleep(1_000)
    await record('diag-page-before-offline-export', true, {
      text: document.body.innerText.slice(0, 300),
    })
    // With the daemon gone the app shows the startup-problem page; its own export button runs the offline path.
    const offlineButton = await waitFor(
      'startup-problem export button',
      () => buttonWithText(t('startupFailure.export')),
      60000
    )
    offlineButton.click()
    // A MutationObserver is used instead of timer polling: a parked WebView throttles timers.
    const feedback = await new Promise<HTMLElement>((resolve, reject) => {
      const probe = () => document.querySelector<HTMLElement>('[role="status"], [role="alert"]')
      const found = probe()
      if (found) return resolve(found)
      const observer = new MutationObserver(() => {
        const el = probe()
        if (el) {
          observer.disconnect()
          resolve(el)
        }
      })
      observer.observe(document.body, { childList: true, subtree: true })
      setTimeout(() => {
        observer.disconnect()
        reject(
          new Error(
            `timeout: offline export feedback; page=${document.body.innerText.slice(0, 200)}; statusNodes=${document.querySelectorAll('[role="status"],[role="alert"]').length}`
          )
        )
      }, 120000)
    })
    const feedbackText = feedback.textContent
    await sleep(1_000)
    await record('diag-export-offline', true, {
      feedback: feedbackText,
      role: feedback.getAttribute('role'),
      page: document.body.innerText.slice(0, 600),
    })
    await record('diag-driver-complete', true)
    return control('exit')
  }
}
