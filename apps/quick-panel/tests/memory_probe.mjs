// Memory probe for the GPUI quick panel: real panel process, synthetic daemon, physical footprint.
//
//   node memory_probe.mjs --binary <uniclip-quick-panel> --images <dir from make_memory_images.py>
//        --out <results.jsonl> [--datasets empty,text,medium,large,animated,rotating] [--cycles 10]
//
// It never reads real history: the panel is pointed at a local synthetic daemon through
// UNICLIPBOARD_DAEMON_BASE_URL. The panel is opened with its global shortcut (ctrl+alt+space, not
// the shortcut of the installed app) and driven with peekaboo, so a visible, unlocked desktop is
// required. The metric is `phys_footprint` from `footprint` (what Activity Monitor shows), never RSS.
import { spawn, execFile } from 'node:child_process'
import { readFileSync, appendFileSync, mkdtempSync, rmSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { crc32 } from 'node:zlib'

const exec = promisify(execFile)
const here = dirname(fileURLToPath(import.meta.url))
const arg = (name, fallback) => {
  const index = process.argv.indexOf(`--${name}`)
  return index > 0 ? process.argv[index + 1] : fallback
}
const binary = arg('binary')
const imageDir = arg('images')
const outFile = arg('out')
const cycles = Number(arg('cycles', 10))
// Keeps the panel running this many seconds after the last measurement, so that it can be
// inspected (for example with vmmap) while it is in its final state.
const hold = Number(arg('hold', 0))
// Like `hold`, but while the panel is open, right after the first show has settled.
const holdShown = Number(arg('hold-shown', 0))
// Fast show / search / hide rounds after the normal run, to exercise cancellation.
const churn = Number(arg('churn', 0))
const datasets = arg('datasets', 'empty,text,medium,large,animated,rotating').split(',')
const peekaboo = process.env.PEEKABOO_BIN ?? '/opt/homebrew/bin/peekaboo'
if (!binary || !imageDir || !outFile) throw new Error('--binary, --images and --out are required')
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const settings = JSON.parse(readFileSync(join(here, 'settings.json'), 'utf8'))
settings.general.language = 'en'

const row = (index, extra) => ({
  entryId: `memory-${index}`,
  contentType: 'text',
  activeTimeMs: Date.now() - index * 60000,
  tags: [],
  textPreview: `Synthetic text entry ${index} with a preview long enough to fill a row`,
  charCount: 60,
  mimeType: 'text/plain',
  fileExtensions: [],
  fileNames: [],
  filePaths: [],
  linkUrls: [],
  sourceDevice: null,
  payloadState: null,
  ...extra,
})
const imageRow = (index, file, mime) => ({
  ...row(index, { contentType: 'image', mimeType: mime, tags: ['image'], charCount: null }),
  textPreview: `Synthetic image ${index}`,
  file,
})
// Bytes of an entry's image. In the `rotating` dataset every search returns entries with new ids,
// and a tEXt chunk after the PNG header gives every image different bytes (so a content-hash cache
// cannot reuse an earlier decode), as the history does when new screenshots keep arriving.
function imageBytes(file, salt) {
  const bytes = readFileSync(join(imageDir, file))
  if (!salt || !file.endsWith('.png')) return bytes
  const body = Buffer.from(`Comment\0${salt}`)
  const chunk = Buffer.alloc(12 + body.length)
  chunk.writeUInt32BE(body.length, 0)
  chunk.write('tEXt', 4)
  body.copy(chunk, 8)
  chunk.writeUInt32BE(crc32(chunk.subarray(4, 8 + body.length)), 8 + body.length)
  return Buffer.concat([bytes.subarray(0, 33), chunk, bytes.subarray(33)])
}
const INLINE_LIMIT = 16 * 1024 // the daemon stores larger images as blobs (inline_threshold_bytes)

function buildDataset(name, generation = 0) {
  const texts = (count, prefix = 't') =>
    Array.from({ length: count }, (_, i) => row(i, { entryId: `${prefix}-${generation}-${i}` }))
  const images = (count, prefix, mime = 'image/png', ext = 'png', pool = count) =>
    Array.from({ length: count }, (_, i) => ({
      ...imageRow(i, `${prefix}-${i % pool}.${ext}`, mime),
      entryId: `img-${generation}-${i}`,
    }))
  switch (name) {
    case 'empty':
      return []
    case 'text':
      return texts(200)
    case 'medium':
      return [...images(50, 'medium'), ...texts(100)]
    case 'large':
      return [...images(8, 'large'), ...texts(200)]
    case 'animated':
      return [
        { ...images(1, 'animated', 'image/gif', 'gif')[0], file: 'animated.gif' },
        ...texts(50),
      ]
    case 'rotating':
      return images(50, 'medium')
    default:
      throw new Error(`unknown dataset ${name}`)
  }
}
const expectedImages = rows => rows.slice(0, 50).filter(r => r.file).length

function startDaemon(name) {
  let rows = buildDataset(name, 0)
  const stats = { searches: 0, resources: 0, blobs: 0, settings: 0 }
  const blobs = new Map()
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://localhost')
    const json = (status, body) => {
      response.writeHead(status, { 'content-type': 'application/json' })
      response.end(JSON.stringify(body))
    }
    if (url.pathname === '/__stats') return json(200, stats)
    if (url.pathname === '/auth/connect')
      return json(200, {
        data: { sessionToken: 's', expiresInSecs: 3600, refreshAtSecs: 3000 },
        ts: Date.now(),
      })
    if (url.pathname === '/settings') {
      stats.settings++
      return json(200, { data: settings, ts: Date.now() })
    }
    if (url.pathname === '/search/tags') return json(200, { data: [], ts: Date.now() })
    if (url.pathname === '/paired-devices') return json(200, { data: [], ts: Date.now() })
    if (url.pathname === '/search/query') {
      if (url.searchParams.get('limit') !== '1') {
        stats.searches++
        // Every search of the rotating dataset is a history with new entries.
        if (name === 'rotating') rows = buildDataset(name, stats.searches)
      }
      const offset = Number(url.searchParams.get('offset') ?? 0)
      const limit = Number(url.searchParams.get('limit') ?? 50)
      const items = rows.slice(offset, offset + limit).map(({ file, ...item }) => item)
      return json(200, {
        data: { items, total: rows.length, hasMore: rows.length > offset + limit, state: 'ready' },
        ts: Date.now(),
      })
    }
    if (url.pathname.startsWith('/clipboard/blobs/')) {
      const bytes = blobs.get(url.pathname.split('/').at(-1))
      if (!bytes) return json(404, {})
      stats.blobs++
      response.writeHead(200, { 'content-type': 'application/octet-stream' })
      return response.end(bytes)
    }
    if (url.pathname.startsWith('/clipboard/entries/')) {
      const id =
        url.pathname.split('/').at(-2) === 'entries'
          ? url.pathname.split('/').at(-1)
          : url.pathname.split('/').at(-2)
      const found = rows.find(r => r.entryId === id)
      if (!found) return json(404, {})
      if (url.pathname.endsWith('/resource')) {
        if (!found.file) return json(404, {})
        stats.resources++
        const bytes = imageBytes(found.file, name === 'rotating' ? found.entryId : '')
        const large = bytes.length > INLINE_LIMIT
        if (large) blobs.set(`blob-${id}`, bytes)
        return json(200, {
          data: {
            blobId: large ? `blob-${id}` : null,
            mimeType: found.mimeType,
            sizeBytes: bytes.length,
            url: null,
            inlineData: large ? null : bytes.toString('base64'),
          },
          ts: Date.now(),
        })
      }
      return json(200, {
        data: {
          id,
          content: found.textPreview,
          sizeBytes: 60,
          createdAtMs: found.activeTimeMs,
          activeTimeMs: found.activeTimeMs,
          mimeType: found.mimeType,
        },
        ts: Date.now(),
      })
    }
    json(404, {})
  })
  return new Promise(resolve =>
    server.listen(0, '127.0.0.1', () =>
      resolve({
        server,
        stats,
        rows: () => rows,
        address: `http://127.0.0.1:${server.address().port}`,
      })
    )
  )
}

let footprintCalls = 0
async function footprint(pid, scratch) {
  // A fresh file for every call, and a failure is a failure: reading an earlier call's file would
  // report an old sample as a new one.
  const file = join(scratch, `fp-${pid}-${++footprintCalls}.json`)
  await exec('footprint', ['-j', file, String(pid)])
  const process_ = JSON.parse(readFileSync(file, 'utf8')).processes[0]
  if (process_.pid !== pid) throw new Error(`footprint reported pid ${process_.pid}, not ${pid}`)
  rmSync(file)
  const category = name => process_.categories[name]?.dirty ?? 0
  const { stdout } = await exec('ps', ['-o', 'rss=', '-p', String(pid)])
  return {
    footprint: process_.footprint,
    peak: process_.auxiliary?.phys_footprint_peak ?? null,
    rssKb: Number(stdout.trim()),
    mallocLarge: category('Malloc Large'),
    mallocSmall: category('Malloc Small'),
    ioAccelerator: category('IOAccelerator (graphics)'),
    ioSurface: category('IOSurface'),
    // Every category with its dirty bytes, to see where memory sits when the named ones are small.
    categories: Object.fromEntries(
      Object.entries(process_.categories).map(([key, value]) => [key, value.dirty ?? 0])
    ),
  }
}

// CPU time the process has used so far, in seconds (`ps` prints [dd-][hh:]mm:ss.cc).
async function cpuSeconds(pid) {
  const { stdout } = await exec('ps', ['-o', 'time=', '-p', String(pid)])
  const parts = stdout.trim().replace(/^(\d+)-/, '$1:').split(':')
  return parts.reduce((total, part) => total * 60 + Number(part), 0)
}

const press = (pid, ...keys) =>
  exec(peekaboo, ['press', ...keys, '--pid', String(pid), '--no-auto-focus', '--no-remote'], {
    timeout: 15_000,
  })
const hotkey = () =>
  exec(peekaboo, ['hotkey', '--keys', 'ctrl,alt,space', '--no-remote'], { timeout: 15_000 })

async function runDataset(name, scratch) {
  const daemon = await startDaemon(name)
  const app = spawn(binary, ['--exit-when-stdin-closes'], {
    env: {
      ...process.env,
      UNICLIPBOARD_DAEMON_BASE_URL: daemon.address,
      UNICLIPBOARD_DAEMON_TOKEN_PATH: join(here, 'fixture-token.txt'),
      UC_GPUI_SHORTCUT: 'ctrl+alt+space',
      UC_GPUI_SCALE: process.env.PROBE_SCALE ?? '1',
    },
    stdio: ['pipe', 'ignore', 'ignore'],
  })
  const record = async (phase, extra = {}) => {
    const sample = await footprint(app.pid, scratch)
    const line = { dataset: name, phase, ts: new Date().toISOString(), ...sample, ...extra }
    appendFileSync(outFile, `${JSON.stringify(line)}\n`)
    console.log(name.padEnd(9), phase.padEnd(26), `${(sample.footprint / 1048576).toFixed(1)} MiB`)
  }
  const until = async (description, predicate, timeout = 30_000) => {
    const end = Date.now() + timeout
    while (Date.now() < end) {
      if (await predicate()) return
      await delay(50)
    }
    throw new Error(`${name}: timed out waiting for ${description}`)
  }
  // The panel has done the work it was asked for when it stops using CPU. Delivery of the images
  // by the daemon is not that: they are decoded afterwards, off the request path, so this waits
  // for three consecutive 400 ms windows with less than 30 ms of CPU time in each.
  const quiet = async () => {
    let still = 0
    let last = await cpuSeconds(app.pid)
    const end = Date.now() + 60_000
    while (still < 3) {
      if (Date.now() > end) throw new Error(`${name}: the panel kept using CPU for 60 s`)
      await delay(400)
      const now = await cpuSeconds(app.pid)
      still = now - last < 0.03 ? still + 1 : 0
      last = now
    }
  }
  // Opens the panel with its shortcut and waits until it has searched again, until every image of
  // the first page has been delivered, and then until it is quiet (see `quiet`). Returns how long
  // that took, in milliseconds: from the shortcut to the panel being idle again.
  const show = async () => {
    const before = { ...daemon.stats }
    const started = Date.now()
    // A shortcut sent while the panel is still hiding can be lost, so it is sent once more.
    for (let attempt = 1; ; attempt++) {
      await hotkey()
      try {
        await until('the panel to search', () => daemon.stats.searches > before.searches, 6000)
        break
      } catch (error) {
        if (attempt === 3) throw error
        await press(app.pid, 'escape')
        await delay(1500)
      }
    }
    const images = expectedImages(daemon.rows())
    await until(
      'the images to be fetched',
      () => daemon.stats.resources - before.resources >= images
    )
    await quiet()
    return Date.now() - started
  }
  const hide = async () => {
    await press(app.pid, 'escape')
  }
  try {
    await delay(4000)
    await record('cold-start-hidden')
    const loadMs = await show()
    await record('first-show', { loadMs, images: expectedImages(daemon.rows()) })
    await delay(10000)
    await record('shown-12s')
    if (holdShown > 0) {
      console.log(`${name}: holding pid ${app.pid} while shown for ${holdShown}s`)
      await delay(holdShown * 1000)
    }
    await hide()
    await delay(5000)
    await record('hidden-5s')
    await delay(30000)
    await record('hidden-35s')
    for (let cycle = 1; cycle <= cycles; cycle++) {
      const cycleMs = await show()
      if (cycle % 5 === 0) await record(`cycle-${cycle}-shown`, { loadMs: cycleMs })
      await hide()
      await delay(cycle % 5 === 0 ? 5000 : 800)
      if (cycle % 5 === 0)
        await record(`cycle-${cycle}-hidden-5s`, { resources: daemon.stats.resources })
    }
    await show()
    await press(app.pid, 'down', '--count', '60')
    await press(app.pid, 'up', '--count', '60')
    await delay(1500)
    await record('after-scroll-shown')
    // Selecting the next row moves the preview to another entry.
    await press(app.pid, 'down', '--count', '1')
    await delay(3000)
    await record('preview-moved')
    await hide()
    await delay(5000)
    await record('hidden-after-preview-5s')
    await delay(30000)
    await record('hidden-after-preview-35s')
    if (churn > 0) {
      // Show, change the search at once (rotating histories answer with new images), and hide
      // before anything has settled; then show again. Cancelled loads and decodes must not leave
      // bitmaps behind or run past the decode limit.
      for (let round = 1; round <= churn; round++) {
        await hotkey()
        await delay(120)
        await exec(peekaboo, ['type', 'a', '--pid', String(app.pid), '--no-auto-focus', '--no-remote'], {
          timeout: 15_000,
        })
        await delay(350)
        await press(app.pid, 'escape')
        await delay(80)
        if (round % 10 === 0) await record(`churn-${round}-hidden-fast`)
      }
      const churnMs = await show()
      await record('churn-final-shown', { loadMs: churnMs })
      await hide()
      await delay(5000)
      await record('churn-final-hidden-5s')
      await delay(30000)
      await record('churn-final-hidden-35s')
    }
    if (hold > 0) {
      console.log(`${name}: holding pid ${app.pid} for ${hold}s`)
      await delay(hold * 1000)
    }
  } finally {
    app.kill('SIGTERM')
    await delay(1000)
    if (app.exitCode === null) app.kill('SIGKILL')
    daemon.server.close()
  }
}

const scratch = mkdtempSync(join(tmpdir(), 'uc-memory-probe-'))
for (const name of datasets) await runDataset(name, scratch)
