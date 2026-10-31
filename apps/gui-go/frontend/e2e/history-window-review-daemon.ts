import { configureStore } from '@reduxjs/toolkit'
/** Production cache/window hooks against a real isolated daemon.
 * Only the host session/event bridge and labelled busy transport outcomes are controlled.
 * Successful HTTP bodies always come from the daemon. Completion gates force race ordering.
 */
import { JSDOM } from 'jsdom'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'
import { Provider } from 'react-redux'
import {
  clearBlobImageCache,
  getBlobImageObjectUrl,
  peekBlobImageObjectUrl,
  retainBlobImage,
  releaseBlobImage,
  historyImageCacheSize,
} from '../src/api/daemon/blob-image-cache'
import { daemonClient } from '../src/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '../src/api/daemon/errors'
import { client } from '../src/api/generated/client.gen'
import { useHistoryWindow } from '../src/hooks/useHistoryWindow'
import { useHistoryWindowImages } from '../src/hooks/useHistoryWindowImages'
import type { DisplayClipboardItem } from '../src/lib/clipboard-entry'
import { clipboardPreviewCache } from '../src/lib/clipboard-preview-cache'
import { daemonWs } from '../src/lib/daemon-ws'

const { base, authorization, thumbnailPaths, mobileUrl, mobileAuthorization } = JSON.parse(
  await Bun.stdin.text()
)
const assert = (value: unknown, message: string) => {
  if (!value) throw new Error(message)
}
const dom = new JSDOM('<div id="root"></div>')
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  IS_REACT_ACT_ENVIRONMENT: true,
})
const sleep = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))
const waitFor = async (probe: () => unknown, message: string) => {
  for (let i = 0; i < 150; i++) {
    if (probe()) return
    await act(async () => {
      await sleep(20)
    })
  }
  throw new Error(message)
}
const selectedCase = process.env.UC_HISTORY_REVIEW_CASE ?? 'all'
let root: ReturnType<typeof createRoot>
let admissionBusy = false
const ids: string[] = thumbnailPaths.map((path: string) => decodeURIComponent(path.split('/')[3]))
let busyPath: string | null = null
const attempts: number[] = []
daemonClient.fetchBlob = async (path, options) => {
  if (path === busyPath) {
    attempts.push(Date.now())
    throw new DaemonApiError(DaemonErrorCode.SERVICE_UNAVAILABLE, 'injected busy transport')
  }
  const response = await fetch(base + path, {
    headers: { Authorization: authorization },
    signal: options?.signal,
  })
  assert(response.ok, `resource HTTP ${response.status}`)
  return response.blob()
}
daemonClient.callSdk = async call => (await call()).data
daemonWs.subscribe = (() => () => {}) as typeof daemonWs.subscribe
daemonWs.onReconnect = () => () => {}
const gates: Array<{ match: string; started: () => void; hold: Promise<void> }> = []
let searchRequests = 0
let resourceRequests = 0
client.setConfig({
  baseUrl: base,
  headers: { Authorization: authorization },
  fetch: async (input, init) => {
    const request = new Request(input, init)
    if (request.url.includes('/search/query')) searchRequests++
    if (request.url.includes('/resource')) resourceRequests++
    const gate = gates.findIndex(item => request.url.includes(item.match))
    const barrier = gate >= 0 ? gates.splice(gate, 1)[0] : undefined
    const response = await fetch(request)
    if (barrier) {
      barrier.started()
      await barrier.hold
    }
    return response
  },
})
function holdNext(match: string) {
  let release!: () => void
  let started!: () => void
  const ready = new Promise<void>(resolve => {
    started = resolve
  })
  const hold = new Promise<void>(resolve => {
    release = resolve
  })
  gates.push({ match, started, hold })
  return { ready, release }
}

if (selectedCase === 'all' || selectedCase === 'cache') {
  // Saturation must preserve a visible URL while evicting a soft buffer lease.
  const paths = Array.from({ length: 50 }, (_, index) => `${thumbnailPaths[0]}?retained=${index}`)
  for (const path of paths) {
    retainBlobImage(path, 'buffer')
    await getBlobImageObjectUrl(path)
  }
  retainBlobImage(paths[0])
  const visibleUrl = peekBlobImageObjectUrl(paths[0])!
  await getBlobImageObjectUrl(`${thumbnailPaths[1]}?extra=1`)
  assert(historyImageCacheSize() === 50, 'cache exceeded its hard capacity')
  assert(
    peekBlobImageObjectUrl(paths[0]) === visibleUrl && (await fetch(visibleUrl)).ok,
    'visible URL was revoked'
  )
  assert(peekBlobImageObjectUrl(paths[1]) === null, 'unused buffer URL was not evicted')
  releaseBlobImage(paths[0])
  for (const path of paths) releaseBlobImage(path, 'buffer')
  clearBlobImageCache()

  // All pinned slots reject admission instead of revoking an active URL or overflowing.
  for (const path of paths) {
    retainBlobImage(path)
    await getBlobImageObjectUrl(path)
  }
  admissionBusy = false
  try {
    await getBlobImageObjectUrl(`${thumbnailPaths[1]}?all-pinned=1`)
  } catch (error) {
    admissionBusy =
      error instanceof DaemonApiError && error.code === DaemonErrorCode.SERVICE_UNAVAILABLE
  }
  assert(
    admissionBusy && historyImageCacheSize() === 50,
    'pinned saturation did not preserve the hard bound'
  )
  assert(
    (await fetch(peekBlobImageObjectUrl(paths[0])!)).ok,
    'pinned saturation revoked a visible URL'
  )
  for (const path of paths) releaseBlobImage(path)
  clearBlobImageCache()
}
if (selectedCase === 'all' || selectedCase === 'preload') {
  // Busy first item cannot prevent later real resources from loading; disposal cancels retries.
  const items = ids.map(id => ({ id, type: 'image' })) as DisplayClipboardItem[]
  function Images() {
    useHistoryWindowImages(items)
    return null
  }
  root = createRoot(document.getElementById('root')!)
  busyPath = thumbnailPaths[0]
  await act(async () => {
    root.render(React.createElement(Images))
  })
  await waitFor(
    () => peekBlobImageObjectUrl(thumbnailPaths[1]) && peekBlobImageObjectUrl(thumbnailPaths[2]),
    'busy image blocked later resources'
  )
  await act(async () => {
    await sleep(1700)
  })
  assert(attempts.length >= 2 && attempts.length <= 3, 'busy retries did not back off')
  assert(attempts[1] - attempts[0] >= 500, 'busy retry had no initial backoff')
  await act(async () => {
    root.unmount()
  })
  const stopped = attempts.length
  await sleep(1100)
  assert(attempts.length === stopped, 'disposed preload continued retrying')
  busyPath = null
  clearBlobImageCache()
}
if (selectedCase === 'all' || selectedCase === 'preview') {
  // Exercise preview Promise identity against actual generated-SDK resource responses.
  const id = ids[0]
  clipboardPreviewCache.clear()
  const oldGate = holdNext('/resource')
  const old = clipboardPreviewCache.get(id)
  await oldGate.ready
  clipboardPreviewCache.invalidate(id)
  const newGate = holdNext('/resource')
  const fresh = clipboardPreviewCache.get(id)
  await newGate.ready
  oldGate.release()
  await old
  assert(clipboardPreviewCache.peek(id) === null, 'obsolete preview repopulated the cache')
  const beforeJoin = resourceRequests
  const joined = clipboardPreviewCache.get(id)
  await sleep(20)
  assert(resourceRequests === beforeJoin, 'obsolete finally deleted the replacement request')
  newGate.release()
  const [freshValue, joinedValue] = await Promise.all([fresh, joined])
  assert(
    freshValue === joinedValue && clipboardPreviewCache.peek(id) === freshValue,
    'new preview request was not shared'
  )
  clipboardPreviewCache.clear()
  for (const dispose of [
    () => clipboardPreviewCache.retain(new Set()),
    () => clipboardPreviewCache.clear(),
  ]) {
    const gate = holdNext('/resource')
    const pending = clipboardPreviewCache.get(id)
    await gate.ready
    dispose()
    gate.release()
    await pending
    assert(clipboardPreviewCache.peek(id) === null, 'disposed preview re-entered the cache')
  }
}
if (selectedCase === 'all' || selectedCase === 'refresh') {
  // Seed real text captures so the window spans more than one cache capacity.
  for (let index = 0; index < 52; index++) {
    const response = await fetch(mobileUrl + '/SyncClipboard.json', {
      method: 'PUT',
      headers: { Authorization: mobileAuthorization, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        type: 'Text',
        text: `window-review-${index}`,
        dataName: '',
        hasData: false,
      }),
    })
    assert(response.ok, `capture HTTP ${response.status}`)
  }
  const model = { query: '' }
  let windowState!: ReturnType<typeof useHistoryWindow>
  function Window() {
    windowState = useHistoryWindow(model)
    return null
  }
  root = createRoot(document.getElementById('root')!)
  const store = configureStore({ reducer: (state = {}) => state })
  await act(async () => {
    root.render(React.createElement(Provider, { store, children: React.createElement(Window) }))
  })
  await waitFor(
    () => windowState?.items.length === 50 && !windowState.isLoading,
    'real window did not load 50 records'
  )
  const refreshGate = holdNext('/search/query')
  const beforeRefresh = searchRequests
  await act(async () => {
    windowState.reportViewport(0, 9)
    windowState.refetch()
  })
  await refreshGate.ready
  const inserted = await fetch(mobileUrl + '/SyncClipboard.json', {
    method: 'PUT',
    headers: { Authorization: mobileAuthorization, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      type: 'Text',
      text: 'window-review-during-refresh',
      dataName: '',
      hasData: false,
    }),
  })
  assert(inserted.ok, 'capture during refresh failed')
  await act(async () => {
    await sleep(1000)
  })
  // An idle load may supersede the pending refresh, but must make a complete authoritative query.
  refreshGate.release()
  await waitFor(
    () => !windowState.isLoading && windowState.items.length === 20,
    'idle refresh did not settle'
  )
  const authoritative = await fetch(base + '/search/query?query=&offset=0&limit=20', {
    headers: { Authorization: authorization },
  }).then(response => response.json())
  assert(
    JSON.stringify(windowState.items.map(item => item.id)) ===
      JSON.stringify(authoritative.data.items.map((item: { entryId: string }) => item.entryId)),
    'refresh retained stale records'
  )
  assert(searchRequests >= beforeRefresh + 2, 'idle/refresh race was not exercised')
  await act(async () => {
    root.unmount()
  })
}
dom.window.close()
console.log(
  JSON.stringify({
    passed: true,
    selectedCase,
    retainedUrlSurvived: selectedCase === 'all' || selectedCase === 'cache',
    allPinnedAdmissionBusy: admissionBusy,
    hardCapacity: 50,
    busyDidNotBlock: selectedCase === 'all' || selectedCase === 'preload',
    busyAttempts: attempts.length,
    busyIntervalsMs: attempts.slice(1).map((time, index) => time - attempts[index]),
    preloadDisposed: selectedCase === 'all' || selectedCase === 'preload',
    previewOldCompletionNotCached: selectedCase === 'all' || selectedCase === 'preview',
    previewReplacementShared: selectedCase === 'all' || selectedCase === 'preview',
    previewRetainAndClearDiscarded: selectedCase === 'all' || selectedCase === 'preview',
    idleRefreshAuthoritative: selectedCase === 'all' || selectedCase === 'refresh',
    realSearchRequests: searchRequests,
    realResourceRequests: resourceRequests,
    hostSessionAndEventsBridge: 'replaced',
    busyResponse: 'injected-only-for-first-thumbnail',
  })
)
