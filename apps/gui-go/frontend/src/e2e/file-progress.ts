import { listOutboundSnapshots } from '@/api/outbound_progress'
// Failure modes: real byte events have no visible progress, progress never changes,
// or the UI reports completion before the receiver has the complete file.
import { daemonWs } from '@/lib/daemon-ws'
import { store } from '@/store'

type Helpers = {
  record: (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
  waitFor: <T>(label: string, probe: () => T | null | undefined | false, ms?: number) => Promise<T>
  reachMainLayout: () => Promise<void>
}

export async function runFileProgress(h: Helpers) {
  await h.reachMainLayout()
  const unsubscribe = daemonWs.subscribe(['file-transfer', 'clipboard'], event => {
    // Persist event kinds and byte counters only; never persist payload content or credentials.
    const p = event.payload as Record<string, unknown> | null
    const counters = Object.fromEntries(
      Object.entries(p ?? {}).filter(
        ([key, value]) => /bytes|total|percent/i.test(key) && typeof value === 'number'
      )
    )
    void h.record('transfer-event', true, { type: event.eventType, counters })
    if (event.eventType === 'file-transfer.outbound_progress_changed') {
      void listOutboundSnapshots()
        .then(snapshots =>
          h.record(
            'outbound-snapshots',
            true,
            snapshots.map(snapshot => ({
              state: snapshot.state,
              freshness: snapshot.freshness,
              items: snapshot.items?.map(item => ({
                bytes: item.completedBytes,
                total: item.totalBytes,
              })),
            }))
          )
        )
        .catch(() => h.record('outbound-snapshots', false))
    }
  })
  await h.record('progress-observer-ready', true)
  let last = ''
  const deadline = Date.now() + 180000
  try {
    while (Date.now() < deadline) {
      const nodes = Array.from(
        document.querySelectorAll<HTMLElement>('[role="progressbar"], [role="status"]')
      )
      const visible = nodes.filter(
        node =>
          node.getBoundingClientRect().width > 0 && getComputedStyle(node).visibility !== 'hidden'
      )
      const percentages = document.body.innerText.match(/\b\d{1,3}%/g) ?? []
      const sample = {
        bars: visible.map(node => ({ role: node.role, value: node.getAttribute('aria-valuenow') })),
        percentages,
        pendingReceives: Object.values(store.getState().fileTransfer.activeTransfers).filter(
          transfer =>
            transfer.status === 'active' && transfer.direction === 'receiving' && !transfer.entryId
        ).length,
      }
      const serial = JSON.stringify(sample)
      if (serial !== last) {
        last = serial
        await h.record('progress-dom', true, sample)
      }
      await new Promise(resolve => setTimeout(resolve, 40))
    }
  } finally {
    unsubscribe()
  }
}
