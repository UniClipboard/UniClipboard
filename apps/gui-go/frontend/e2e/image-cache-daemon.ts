/** Nonvisual integration against the daemon started by thumbnail_resource_run.py.
 * The real frontend cache and original-image loader run in Bun. The native host
 * session bridge is replaced by the already-authorized loopback request.
 */
import { createHash } from 'node:crypto'
import { JSDOM } from 'jsdom'
import React, { act } from 'react'
import { createRoot } from 'react-dom/client'
import {
  clearBlobImageCache,
  getBlobImageObjectUrl,
  peekBlobImageObjectUrl,
} from '../src/api/daemon/blob-image-cache'
import { daemonClient } from '../src/api/daemon/client'
import { DaemonApiError, DaemonErrorCode } from '../src/api/daemon/errors'
import { useBlobImageObjectUrl } from '../src/hooks/useBlobImageObjectUrl'
import { loadImageBlob } from '../src/lib/image-handoff'

const input = await Bun.stdin.text()
const { base, authorization, originalPath, thumbnailPath, expectedSha256 } = JSON.parse(input)
const assert = (condition: unknown, message: string) => {
  if (!condition) throw new Error(message)
}
let pauseFirst = false
let release: (() => void) | undefined
let started: (() => void) | undefined
const startedFirst = new Promise<void>(resolve => {
  started = resolve
})
let requests = 0
let busyResponses = 0
// The session bridge is host-owned. Keep HTTP and the production cache real.
daemonClient.fetchBlob = async (path: string): Promise<Blob> => {
  requests++
  if (busyResponses > 0) {
    busyResponses--
    throw new DaemonApiError(DaemonErrorCode.SERVICE_UNAVAILABLE, 'thumbnail busy')
  }
  if (pauseFirst) {
    pauseFirst = false
    started?.()
    await new Promise<void>(resolve => {
      release = resolve
    })
  }
  const response = await fetch(base + path, { headers: { Authorization: authorization } })
  if (!response.ok) throw new Error(`resource HTTP ${response.status}`)
  return response.blob()
}

const original = await loadImageBlob(originalPath)
const digest = createHash('sha256')
  .update(new Uint8Array(await original.arrayBuffer()))
  .digest('hex')
assert(digest === expectedSha256, 'image action did not receive the original bytes')
const originalUrl = peekBlobImageObjectUrl(originalPath)
assert(originalUrl, 'original URL was not cached')
clearBlobImageCache()

// Exercise the real React hook in a nonvisual DOM. Only the busy response and
// host session bridge are simulated; the retry fetches from the real daemon.
const dom = new JSDOM('<div id="root"></div>')
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  IS_REACT_ACT_ENVIRONMENT: true,
})
const root = createRoot(dom.window.document.getElementById('root')!)
let hookUrl: string | null = null
function Probe({ enabled }: { enabled: boolean }) {
  hookUrl = useBlobImageObjectUrl(`${thumbnailPath}`, enabled)
  return null
}
busyResponses = 1
await act(async () => {
  root.render(React.createElement(Probe, { enabled: true }))
})
await act(async () => {
  await new Promise(resolve => setTimeout(resolve, 1000))
})
assert(hookUrl, 'retry did not load the thumbnail')
await act(async () => {
  root.render(React.createElement(Probe, { enabled: false }))
})
clearBlobImageCache()
busyResponses = 2
await act(async () => {
  root.render(React.createElement(Probe, { enabled: true }))
})
const beforeHidden = requests
await act(async () => {
  root.render(React.createElement(Probe, { enabled: false }))
})
await act(async () => {
  await new Promise(resolve => setTimeout(resolve, 750))
})
assert(requests === beforeHidden, 'retry continued after hiding')
busyResponses = 0
await act(async () => {
  root.unmount()
})
clearBlobImageCache()
dom.window.close()
assert(peekBlobImageObjectUrl(originalPath) === null, 'clear retained an original URL')
let revoked = false
try {
  await fetch(originalUrl!)
} catch {
  revoked = true
}
assert(revoked, 'cleared object URL remains usable')
const reloaded = await loadImageBlob(originalPath)
assert(
  createHash('sha256')
    .update(new Uint8Array(await reloaded.arrayBuffer()))
    .digest('hex') === expectedSha256,
  'original could not be loaded after clear'
)
clearBlobImageCache()

pauseFirst = true
const old = getBlobImageObjectUrl(thumbnailPath)
await startedFirst
clearBlobImageCache()
const fresh = await getBlobImageObjectUrl(thumbnailPath)
assert(fresh, 'new request after clear failed')
release?.()
assert((await old) === null, 'old in-flight request restored a cleared URL')
assert(peekBlobImageObjectUrl(thumbnailPath) === fresh, 'old request replaced the new URL')
clearBlobImageCache()

for (let i = 0; i < 129; i++) {
  assert(await getBlobImageObjectUrl(`${thumbnailPath}?e2e=${i}`), 'thumbnail fetch failed')
}
assert(peekBlobImageObjectUrl(`${thumbnailPath}?e2e=0`) === null, 'oldest entry was not evicted')
assert(peekBlobImageObjectUrl(`${thumbnailPath}?e2e=128`) !== null, 'newest entry missing')
clearBlobImageCache()
console.log(
  JSON.stringify({
    passed: true,
    originalSha256: digest,
    revoked: true,
    reloadAfterClear: true,
    oldRequestDiscarded: true,
    newRequestPreserved: true,
    lruEviction: true,
    retryAfterBusy: true,
    hiddenRetryCancelled: true,
    requests,
    runtime: 'bun',
    hostSessionBridge: 'replaced',
  })
)
