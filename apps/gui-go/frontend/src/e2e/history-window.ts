import { Call, Window } from '@wailsio/runtime'
import { daemonClient } from '@/api/daemon/client'
import i18n from '@/i18n'

interface ScenarioContext {
  reachMainLayout: () => Promise<void>
  record: (step: string, ok: boolean, detail?: unknown) => Promise<unknown>
  control: (action: string) => Promise<unknown>
  waitFor: <T>(label: string, probe: () => T | null | undefined | false, ms?: number) => Promise<T>
  historyRows: () => HTMLElement[]
  sleep: (ms: number) => Promise<unknown>
  $: (selector: string) => HTMLElement | null
}

export async function runHistoryWindowScenario({
  reachMainLayout,
  record,
  control,
  waitFor,
  historyRows,
  sleep,
  $,
}: ScenarioContext) {
  await record('window-begin', true, {
    visibility: document.visibilityState,
    path: location.pathname,
  })
  await reachMainLayout()
  await waitFor('visible history window', () => document.visibilityState === 'visible')
  const configuration = JSON.parse(String(await Call.ByName('main.EvidenceService.Secret'))) as {
    ids: string[]
    grid: boolean
    largeViewport?: boolean
  }
  const mode = configuration.grid ? 'grid' : 'list'
  $(
    `[role="radio"][aria-label="${i18n.t(configuration.grid ? 'history.kit.viewGrid' : 'history.kit.viewList')}"]`
  )!.click()
  await waitFor(`${mode} mode`, () =>
    configuration.grid
      ? document.querySelector('.mac-grid, .adw-grid')
      : document.querySelector('[data-item-id]') && !document.querySelector('.mac-grid, .adw-grid')
  )
  await record('window-view', true, { mode })
  const scroller = await waitFor('bounded history', () =>
    document.querySelector<HTMLElement>('[data-history-window-scroller]')
  )
  const cache = await import('@/api/daemon/blob-image-cache')
  let maxRecords = 0
  let maxImages = 0
  const check = () => {
    const count = Number(document.documentElement.dataset.historyWindowCount ?? 0)
    maxRecords = Math.max(
      maxRecords,
      count,
      Number(document.documentElement.dataset.historyWindowPeakCount ?? 0)
    )
    maxImages = Math.max(maxImages, cache.historyImageCacheSize())
    if (maxRecords > 50 || maxImages > 51)
      throw new Error('History exceeded 50 records or 51 image URLs')
  }
  const visible = () => {
    const bounds = scroller.getBoundingClientRect()
    return historyRows().filter(row => {
      const rect = row.getBoundingClientRect()
      return rect.bottom > bounds.top && rect.top < bounds.bottom
    })
  }
  const anchor = () => {
    const row = visible()[0]
    return { id: row.dataset.itemId!, top: row.getBoundingClientRect().top }
  }
  if (configuration.largeViewport) {
    await Window.SetSize(2400, 1800)
    const slider = document.querySelector<HTMLInputElement>('[role="slider"], input[type="range"]')
    if (slider) {
      slider.focus()
      slider.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }))
    }
    await waitFor(
      'large viewport records loaded',
      () => Number(document.documentElement.dataset.historyWindowCount) === 50
    )
    const nativeOverflow = scroller.scrollHeight > scroller.clientHeight + 1
    const offsets: number[] = []
    for (const direction of [1, -1]) {
      for (let step = 0; step < 9; step++) {
        const before = Number(document.documentElement.dataset.historyWindowOffset)
        const target = Math.max(0, Math.min(70, before + direction * 10))
        // Synthetic wheel events need an explicit native-position update when overflow exists.
        const wheel = new WheelEvent('wheel', {
          deltaY: direction * 100,
          bubbles: true,
          cancelable: true,
        })
        scroller.dispatchEvent(wheel)
        if (nativeOverflow) {
          if (wheel.defaultPrevented)
            throw new Error('Overflow wheel also triggered explicit navigation')
          scroller.scrollTop = direction > 0 ? scroller.scrollHeight - scroller.clientHeight : 0
        }
        await waitFor('large viewport navigation', () =>
          nativeOverflow
            ? visible().length > 0 &&
              (direction > 0
                ? Number(document.documentElement.dataset.historyWindowOffset) > 0
                : Number(document.documentElement.dataset.historyWindowOffset) === 0)
            : Number(document.documentElement.dataset.historyWindowOffset) === target &&
              visible().length === 50
        )
        await sleep(850)
        check()
        offsets.push(Number(document.documentElement.dataset.historyWindowOffset))
      }
    }
    const grid = scroller.closest<HTMLElement>('[role="listbox"]')!
    for (const [key, offset] of [
      ['PageDown', 10],
      ['PageUp', 0],
    ] as const) {
      const keyboard = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
      grid.dispatchEvent(keyboard)
      if (nativeOverflow) {
        if (keyboard.defaultPrevented)
          throw new Error('Overflow Page key also triggered explicit navigation')
        scroller.scrollTop = key === 'PageDown' ? scroller.scrollHeight - scroller.clientHeight : 0
      }
      await waitFor('large viewport keyboard navigation', () =>
        nativeOverflow
          ? visible().length > 0 &&
            (offset > 0
              ? Number(document.documentElement.dataset.historyWindowOffset) > 0
              : Number(document.documentElement.dataset.historyWindowOffset) === 0)
          : Number(document.documentElement.dataset.historyWindowOffset) === offset &&
            visible().length === 50
      )
      check()
    }
    await waitFor(
      'large viewport thumbnails decoded',
      () =>
        visible().every(row => {
          const image = row.querySelector<HTMLImageElement>('img')
          return image?.complete && image.naturalWidth > 0
        }),
      15000
    )
    check()
    const passed =
      Math.max(...offsets) > 0 && offsets.at(-1) === 0 && maxRecords === 50 && maxImages > 0
    await record('window-large-viewport', passed, {
      offsets,
      maxRecords,
      maxImages,
      visible: visible().length,
      scrollHeight: scroller.scrollHeight,
      clientHeight: scroller.clientHeight,
      innerWidth,
      innerHeight,
      visibility: document.visibilityState,
      nativeOverflow,
      input:
        'scripted wheel and PageUp/PageDown; native scroll position advances only with overflow',
    })
    if (!passed) throw new Error('Large viewport failed bidirectional navigation')
    await control('exit')
    return
  }
  const verifyIdle = async () => {
    await sleep(2000)
    check()
    const indices = visible().map(row => configuration.ids.indexOf(row.dataset.itemId!))
    const first = Math.max(0, Math.min(...indices) - 10)
    const expected = Math.min(
      50,
      Math.min(configuration.ids.length, Math.max(...indices) + 11) - first
    )
    const count = Number(document.documentElement.dataset.historyWindowCount)
    await waitFor(
      'idle thumbnails ready',
      () =>
        cache.historyImageCachePaths().filter(path => path.endsWith('/thumbnail')).length >=
          count &&
        cache.historyImageCachePaths().filter(path => path.endsWith('/thumbnail')).length <=
          count + 1,
      15000
    )
    await record('window-image-readiness', true, {
      visibility: document.visibilityState,
      scrollTop: scroller.scrollTop,
      bounds: scroller.getBoundingClientRect().toJSON(),
      rows: visible().map(row => {
        const image = row.querySelector<HTMLImageElement>('img')
        const tile = row.querySelector<HTMLElement>('[role="img"]')
        return {
          id: row.dataset.itemId,
          row: row.getBoundingClientRect().toJSON(),
          tile: tile?.getBoundingClientRect().toJSON(),
          hasImage: !!image,
          complete: image?.complete,
          naturalWidth: image?.naturalWidth,
        }
      }),
    })
    await waitFor(
      'visible thumbnails decoded',
      () => {
        const rows = visible()
        return (
          rows.length > 0 &&
          rows.every(row => {
            const tile = row.querySelector<HTMLElement>('[role="img"]')
            if (!tile) return false
            const rect = tile.getBoundingClientRect()
            const bounds = scroller.getBoundingClientRect()
            // A row border may intersect while its thumbnail remains clipped outside the scroller.
            if (rect.bottom <= bounds.top || rect.top >= bounds.bottom) return true
            const image = row.querySelector<HTMLImageElement>('img')
            return image?.complete && image.naturalWidth > 0
          })
        )
      },
      15000
    )
    check()
    if (count !== expected)
      throw new Error(`Idle window has ${count} records, expected ${expected}`)
    return count
  }
  const mutate = async () => {
    const beforeInsert = anchor()
    const beforeNodes = visible().map(row => ({ id: row.dataset.itemId!, row }))
    const retainedNodes = () =>
      beforeNodes.every(({ id, row }) => document.querySelector(`[data-item-id="${id}"]`) === row)
    const evidence = async (stage: string) =>
      record('window-mutation', true, {
        stage,
        anchor: beforeInsert,
        top: document
          .querySelector<HTMLElement>(`[data-item-id="${beforeInsert.id}"]`)
          ?.getBoundingClientRect().top,
        scrollTop: scroller.scrollTop,
        offset: document.documentElement.dataset.historyWindowOffset,
        count: document.documentElement.dataset.historyWindowCount,
        total: document.documentElement.dataset.historyWindowTotal,
      })
    await evidence('before-insert')
    await record('window-insert-request', true)
    await waitFor(
      'inserted total',
      () => Number(document.documentElement.dataset.historyWindowTotal) === 121,
      15000
    )
    await sleep(2500)
    check()
    const insertedAnchor = document.querySelector<HTMLElement>(
      `[data-item-id="${beforeInsert.id}"]`
    )
    const insertDelta = insertedAnchor
      ? Math.abs(insertedAnchor.getBoundingClientRect().top - beforeInsert.top)
      : Infinity
    await evidence('after-insert')
    if (!retainedNodes()) throw new Error('Insertion remounted a retained visible record')
    if (insertDelta > 2) throw new Error(`Insertion moved anchor by ${insertDelta}px`)
    const head = await daemonClient.request<{ data: { items: Array<{ entryId: string }> } }>(
      '/search/query?query=&limit=1&offset=0'
    )
    document.dispatchEvent(
      new CustomEvent('uc-e2e-history-delete', { detail: head.data.items[0].entryId })
    )
    const dialog = await waitFor('delete confirmation', () =>
      document.querySelector<HTMLElement>('[role="alertdialog"]')
    )
    const confirm = Array.from(dialog.querySelectorAll<HTMLButtonElement>('button')).find(
      button => button.textContent === i18n.t('clipboard.deleteLabel')
    )
    if (!confirm) throw new Error('Missing delete confirmation button')
    await evidence('dialog-open')
    confirm.click()
    await waitFor(
      'deleted total',
      () => Number(document.documentElement.dataset.historyWindowTotal) === 120,
      15000
    )
    await sleep(2500)
    check()
    const deletedAnchor = document.querySelector<HTMLElement>(`[data-item-id="${beforeInsert.id}"]`)
    const deleteDelta = deletedAnchor
      ? Math.abs(deletedAnchor.getBoundingClientRect().top - beforeInsert.top)
      : Infinity
    await evidence('after-delete')
    if (!retainedNodes()) throw new Error('Deletion remounted a retained visible record')
    if (deleteDelta > 2) throw new Error(`Deletion moved anchor by ${deleteDelta}px`)

    return { insertDelta, deleteDelta }
  }
  // Exercise continuous input without a settle loop between movements. Sampling
  // every frame catches both intermediate window publication and thumb reversal.
  await sleep(2000)
  const initialRow = visible()[0]
  initialRow.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }))
  if (configuration.grid) initialRow.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
  const selectedId = initialRow.dataset.itemId!
  await waitFor(
    'selected detail before fast scroll',
    () =>
      document.querySelector<HTMLElement>('[data-testid="clipboard-detail"]')?.dataset.entryId ===
      selectedId
  )
  await sleep(100)
  const samples: Array<{ ratio: number; absolute: number | null; total: number; offset: number }> =
    []
  let selectedEvicted = false
  for (let step = 0; step < 90; step++) {
    scroller.scrollTop += Math.max(1, (scroller.scrollHeight - scroller.clientHeight) / 90)
    await sleep(16)
    check()
    const ratio = scroller.scrollTop / Math.max(1, scroller.scrollHeight - scroller.clientHeight)
    const previous = Math.max(0, ...samples.map(sample => sample.ratio))
    if (ratio + 0.01 < previous)
      throw new Error(`Fast scroll thumb reversed: ${previous} -> ${ratio}`)
    const rows = visible()
    const absolute = rows.length ? configuration.ids.indexOf(rows[0].dataset.itemId!) : null
    const total = Number(document.documentElement.dataset.historyWindowTotal)
    if (absolute !== null && absolute >= 0) {
      const tolerance = Math.max(0.04, rows.length / total + 0.03)
      if (Math.abs(ratio - absolute / total) > tolerance)
        throw new Error(
          `Fast scroll position disagrees with result rank: ${ratio}, ${absolute}/${total}`
        )
    }
    const ids = JSON.parse(document.documentElement.dataset.historyWindowIds ?? '[]') as string[]
    selectedEvicted ||= !ids.includes(selectedId)
    const detailId = document.querySelector<HTMLElement>('[data-testid="clipboard-detail"]')
      ?.dataset.entryId
    if (detailId !== selectedId)
      throw new Error('Selection/detail changed during fast scroll eviction')
    samples.push({
      ratio,
      absolute,
      total,
      offset: Number(document.documentElement.dataset.historyWindowOffset),
    })
  }
  await waitFor('fast scroll rows loaded', () => visible().length > 0)
  const fastPassed =
    selectedEvicted && samples.filter(sample => sample.absolute !== null).length >= 10
  await record('window-fast-scroll', fastPassed, {
    samples,
    selectedId,
    selectedEvicted,
    maxRecords,
    maxImages,
  })
  if (!fastPassed) throw new Error('Fast scroll did not exercise selected-record eviction')
  scroller.scrollTop = 0
  await waitFor(
    'return to head after fast scroll',
    () => Number(document.documentElement.dataset.historyWindowOffset) === 0 && visible().length > 0
  )

  let deepMutation: { insertDelta: number; deleteDelta: number } | null = null
  const offsets: number[] = []
  const positions: number[] = []
  await sleep(2000)
  for (const direction of [1, -1]) {
    for (let step = 0; step < 6; step++) {
      const before = Number(document.documentElement.dataset.historyWindowOffset ?? 0)
      scroller.scrollTop = direction > 0 ? scroller.scrollHeight - scroller.clientHeight - 1 : 1
      for (let wait = 0; wait < 40; wait++) {
        await sleep(100)
        check()
      }
      const offset = Number(document.documentElement.dataset.historyWindowOffset ?? 0)
      offsets.push(offset)
      const position = configuration.ids.indexOf(anchor().id)
      const previous = positions.at(-1)
      if (
        previous !== undefined &&
        ((direction > 0 && position < previous) || (direction < 0 && position > previous))
      )
        throw new Error(`Scroll reversed: ${previous} -> ${position}`)
      positions.push(position)
      await record('window-scroll', true, {
        direction,
        before,
        offset,
        count: document.documentElement.dataset.historyWindowCount,
      })
    }
    if (direction > 0) {
      await verifyIdle()
      deepMutation = await mutate()
    }
  }
  const idleCount = await verifyIdle()
  const ids = new Set<string>(JSON.parse(document.documentElement.dataset.historyWindowIds ?? '[]'))
  const obsolete = cache.historyImageCachePaths().filter(path => {
    const match = path.match(/entries\/([^/]+)\/thumbnail/)
    return (
      match &&
      !ids.has(decodeURIComponent(match[1])) &&
      decodeURIComponent(match[1]) !== document.documentElement.dataset.historySelectedId
    )
  })
  if (obsolete.length) throw new Error('Evicted record retained thumbnail URL')
  const { insertDelta, deleteDelta } = await mutate()
  const passed =
    maxImages > 0 && offsets.some(offset => offset > 0) && offsets.at(-1)! < Math.max(...offsets)
  await record('image-memory-all-ready', passed, {
    visibility: document.visibilityState,
    view: document.querySelector('.mac-grid, .adw-grid') ? 'grid' : 'list',
    maxRecords,
    maxImages,
    deepMutation,
    offsets,
    positions,
    idleCount,
    visible: visible().length,
    obsolete: obsolete.length,
    insertDelta,
    deleteDelta,
  })
  if (!passed) throw new Error('Bidirectional history window did not move')
  for (const step of [
    'image-memory-loaded',
    'image-memory-scroll-0',
    'image-memory-scroll-1',
    'image-memory-scroll-2',
    'image-memory-idle',
  ])
    await record(step, true, {
      maxRecords,
      maxImages,
      offsets,
      positions,
      idleCount,
      insertDelta,
      deleteDelta,
    })
  await sleep(1000)
  await control('exit')
}
