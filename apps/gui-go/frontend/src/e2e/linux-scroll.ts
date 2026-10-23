import { EffectsMode } from '@host/models'
import { daemonClient } from '@/api/daemon/client'
import { daemonWs } from '@/lib/daemon-ws'
import { commands } from '@/lib/ipc'
import { saveSettingsPatch } from '@/lib/settings-sync'

type RecordStep = (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
type Control = (action: string) => Promise<unknown>

/** Measure the real history page; no physical input or clipboard access is generated here. */
export async function runLinuxScroll(record: RecordStep, control: Control) {
  let snapshot = false
  const unsubscribe = daemonWs.subscribe(['status'], event => {
    if (event.eventType === 'status.snapshot') snapshot = true
  })
  try {
    const deadline = Date.now() + 15000
    while (!snapshot && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 50))
    await record('scroll-ws-connected', snapshot)
    if (!snapshot) throw new Error('No daemon WebSocket snapshot')
  } finally {
    unsubscribe()
  }
  const entries = await daemonClient.request<{ data: unknown[] }>(
    '/clipboard/entries?limit=100&offset=0'
  )
  await record('scroll-daemon-history', entries.data.length >= 80, { count: entries.data.length })
  if (entries.data.length < 80) throw new Error('Seeded history is missing')
  await commands.unlockContent({ passphrase: 'gui-go-synthetic-passphrase' })
  await commands.setVisualEffectsMode(EffectsMode.EffectsModeSmooth)
  await new Promise(resolve => setTimeout(resolve, 500))
  const viewportDeadline = Date.now() + 15000
  let scroller = document.querySelector<HTMLElement>('.adw-glist-scroller')
  while ((!scroller || scroller.clientHeight < 100) && Date.now() < viewportDeadline) {
    await new Promise(resolve => setTimeout(resolve, 50))
    scroller = document.querySelector<HTMLElement>('.adw-glist-scroller')
  }
  if (!scroller || scroller.clientHeight < 100) throw new Error('History viewport is absent')
  await record('scroll-real-history-mounted', true, {
    path: location.pathname,
    skin: document.querySelector('[data-ui-skin]')?.getAttribute('data-ui-skin'),
    viewportHeight: scroller.clientHeight,
    viewportWidth: scroller.clientWidth,
    scrollHeight: scroller.scrollHeight,
    lowEffects: document.documentElement.dataset.ucLowEffects,
    reduceMotion: document.documentElement.dataset.ucReduceMotion,
    visibility: document.visibilityState,
    devicePixelRatio,
  })
  for (const scheme of ['light', 'dark'] as const) {
    // Applied by the normal host setting path, not fixture-only CSS.
    await saveSettingsPatch({ general: { theme: scheme } })
    scroller.scrollTop = 0
    await new Promise(resolve => setTimeout(resolve, 500))
    const appliedScheme = document.querySelector('[data-adw-theme]')?.getAttribute('data-adw-theme')
    await record(`scroll-theme-${scheme}`, appliedScheme === scheme, { appliedScheme })
    const intervals: number[] = []
    let previous = performance.now()
    let maxPosition = 0
    let maxMounted = 0
    for (let index = 0; index < 240; index++) {
      await new Promise(requestAnimationFrame)
      const now = performance.now()
      intervals.push(now - previous)
      previous = now
      scroller.scrollTop += index < 120 ? 12 : -12
      maxPosition = Math.max(maxPosition, scroller.scrollTop)
      maxMounted = Math.max(maxMounted, scroller.querySelectorAll('[data-item-id]').length)
    }
    intervals.sort((a, b) => a - b)
    await record(`scroll-measurement-${scheme}`, maxPosition > 600 && maxMounted < 80, {
      input: 'scripted scrollTop on real history; not physical touchpad input',
      frames: intervals.length,
      p50Ms: intervals[120],
      p95Ms: intervals[228],
      maxMs: intervals.at(-1),
      over25Ms: intervals.filter(ms => ms > 25).length,
      maxPosition,
      maxMounted,
    })
  }
  await control('show-quick-panel')
  await control('dismiss-quick-panel')
  await record('linux-scroll-complete', true)
  await control('exit')
}
