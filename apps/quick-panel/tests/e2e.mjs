import assert from 'node:assert/strict'
import { spawn, execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtemp, mkdir, rm, access, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve, dirname } from 'node:path'
// Black-box client E2E: real GPUI process, OS input, HTTP fixture and a dedicated paste receiver.
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const exec = promisify(execFile)
const directory = dirname(fileURLToPath(import.meta.url))
const binary = resolve(
  process.env.UC_GPUI_E2E_BINARY ?? join(directory, '../../../target/debug/uniclip-quick-panel')
)
const peekaboo = process.env.PEEKABOO_BIN ?? '/opt/homebrew/bin/peekaboo'
const delay = ms => new Promise(resolve => setTimeout(resolve, ms))
const digest = text => createHash('sha256').update(text).digest('hex')

async function until(description, predicate, timeout = 10_000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const result = await predicate()
    if (result) return result
    await delay(60)
  }
  throw new Error(`Timed out: ${description}`)
}
function launch(command, args, env = process.env) {
  const child = spawn(command, args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
  child.output = ''
  child.errors = ''
  child.stdout.setEncoding('utf8').on('data', data => {
    child.output += data
  })
  child.stderr.setEncoding('utf8').on('data', data => {
    child.errors += data
  })
  child.on('error', error => {
    child.errors += error.message
  })
  return child
}
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return
  const exited = new Promise(resolve => child.once('exit', resolve))
  child.kill('SIGTERM')
  await Promise.race([exited, delay(1500)])
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGKILL')
    await exited
  }
}

// These tests intentionally require a visible, unlocked macOS desktop; failure is not silently skipped.
test('GPUI quick panel native end-to-end', { timeout: 180_000 }, async t => {
  assert.equal(process.platform, 'darwin', 'Native E2E requires macOS')
  await access(binary)
  t.diagnostic(`Binary: ${binary}; built ${(await stat(binary)).mtime.toISOString()}`)
  await access(peekaboo)
  const temporary = await mkdtemp(join(tmpdir(), 'uc-gpui-e2e-'))
  const artifacts = resolve(process.env.UC_GPUI_E2E_ARTIFACTS ?? temporary)
  await mkdir(artifacts, { recursive: true })
  let fixture, target, app
  let address
  let restoreBaseline = 0,
    textBaseline = 0,
    deletedBaseline = 0
  const events = () =>
    target.output
      .split('\n')
      .filter(Boolean)
      .map(line => JSON.parse(line))
  const state = async () => (await fetch(`${address}/__test/state`)).json()
  // The unfiltered list the panel shows first, read from the fixture in the panel's own order, so
  // a test can name "the row with digit 2" without hard-coding the fixture's data.
  const defaultRows = async () =>
    (
      await (
        await fetch(`${address}/search/query?query=&limit=50&offset=0`, {
          headers: { authorization: 'Session fixture-session' },
        })
      ).json()
    ).data.items
  const input = async (command, args) => {
    assert.equal(app.exitCode, null, `GPUI exited: ${app.errors}`)
    await exec(
      peekaboo,
      [command, ...args, '--pid', String(app.pid), '--no-auto-focus', '--no-remote'],
      { timeout: 10_000 }
    )
  }
  const key = keys => input('press', [keys])
  const hotkey = keys => input('hotkey', ['--keys', keys])
  const text = async value => {
    // Seed ASCII before native paste so the legacy empty-query paste shortcut cannot intercept it.
    await input('type', ['seed'])
    await hotkey('cmd,a')
    await input('paste', ['--text', value])
  }
  // Waits for a search with these conditions. `time` is a predicate on the range the panel sent
  // (`{ fromMs, toMs }`); without it the request must carry no time range.
  const query = async (expected, after = -1) =>
    until(`query ${JSON.stringify(expected)}`, async () => {
      const result = await state()
      if (result.searches <= after) return false
      const last = result.lastSearch
      const same = (a, b) => JSON.stringify([...a].sort()) === JSON.stringify([...b].sort())
      return (
        last &&
        last.query === expected.query &&
        same(last.types, expected.types ?? []) &&
        same(last.tags, expected.tags ?? []) &&
        same(last.sources, expected.sources ?? []) &&
        (expected.time
          ? last.fromMs !== null && expected.time(last)
          : last.fromMs === null && last.toMs === null) &&
        result
      )
    })
  const noPaste = async () => {
    // Wait beyond the panel's 80 ms delayed paste and 300 ms search debounce.
    await delay(400)
    assert.equal(
      (await state()).restores.length,
      restoreBaseline,
      'Selecting a filter must not restore a clipboard entry'
    )
    assert.equal(
      events().filter(e => e.event === 'text').length,
      textBaseline,
      'Filter selection must not type into the paste target'
    )
  }
  const windowInfo = async () => {
    const { stdout } = await exec(peekaboo, [
      'list',
      'windows',
      '--pid',
      String(app.pid),
      '--no-remote',
      '--json',
    ])
    return JSON.parse(stdout).data.windows.find(window => window.title === 'UniClipboard History')
  }
  const clickAt = async (x, y) => {
    const window = await windowInfo()
    const [origin] = window.bounds
    const before = events().length
    target.stdin.write(`click ${origin[0] + x} ${origin[1] + y} ${app.pid}\n`)
    const click = await until('guarded native click', () =>
      events()
        .slice(before)
        .find(e => e.event === 'click')
    )
    if (!click.ok) {
      // Name what is on top of the point, so a failure says which window took it.
      const swift =
        'import CoreGraphics;import Foundation;let w=CGWindowListCopyWindowInfo([.optionOnScreenOnly,.excludeDesktopElements],kCGNullWindowID) as! [[String:Any]];for i in w{print(i[kCGWindowOwnerPID as String] ?? "",i[kCGWindowLayer as String] ?? "",i[kCGWindowName as String] ?? "",i[kCGWindowBounds as String] ?? "")}'
      const { stdout } = await exec('swift', ['-e', swift]).catch(error => ({
        stdout: String(error),
      }))
      await writeFile(
        join(artifacts, 'click-windows.txt'),
        `point ${origin[0] + x},${origin[1] + y}; panel pid ${app.pid}; bounds ${JSON.stringify(window.bounds)}\n${stdout}`
      )
    }
    assert.equal(click.ok, true, 'The test panel must own the click point')
  }
  const fresh = async () => {
    await stop(app)
    const count = events().length
    target.stdin.write('reset\nfront\n')
    await until('paste target is active', () =>
      events()
        .slice(count)
        .some(e => e.event === 'front' && e.active)
    )
    const before = await state()
    restoreBaseline = before.restores.length
    deletedBaseline = before.deleted.length
    textBaseline = events().filter(e => e.event === 'text').length
    // Started the way the GUI starts it: with the flag, and with stdin and stdout piped.
    app = launch(binary, ['--exit-when-stdin-closes'], {
      ...process.env,
      UNICLIPBOARD_DAEMON_BASE_URL: address,
      UNICLIPBOARD_DAEMON_TOKEN_PATH: join(directory, 'fixture-token.txt'),
      UC_GPUI_SHORTCUT: 'ctrl+alt+space',
      UC_GPUI_SCALE: '1',
    })
    await query({ query: '' }, before.searches)
    await until('settings and tags loaded', async () => {
      const value = await state()
      return value.settingsReads > before.settingsReads && value.tagsReads > before.tagsReads
    })
    // The panel starts hidden. Open it the way a user does, with its global shortcut; showing it
    // reads the settings and searches again, which is how the test notices it is open.
    const hidden = await state()
    await exec(peekaboo, ['hotkey', '--keys', 'ctrl,alt,space', '--no-remote'], { timeout: 10_000 })
    await until(
      'panel opened by its shortcut',
      async () => (await state()).settingsReads > hidden.settingsReads
    )
    await query({ query: '' }, hidden.searches)
  }
  // `UC_GPUI_E2E_ONLY` runs just the flows whose name contains the text (case-insensitive; several
  // words separated by `|`). Setup still happens once; the other flows are reported as skipped.
  const only = (process.env.UC_GPUI_E2E_ONLY ?? '')
    .split('|')
    .map(word => word.trim().toLowerCase())
    .filter(Boolean)
  const run = async (name, action) => {
    if (only.length > 0 && !only.some(word => name.toLowerCase().includes(word))) {
      await t.test(name, { skip: 'not selected by UC_GPUI_E2E_ONLY' }, () => {})
      return
    }
    await t.test(name, async () => {
      await fresh()
      try {
        await action()
      } catch (error) {
        const window = await windowInfo().catch(() => null)
        if (window)
          await exec('/usr/sbin/screencapture', [
            '-x',
            '-o',
            '-l',
            String(window.window_id),
            join(artifacts, `${name.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.png`),
          ]).catch(() => {})
        await writeFile(
          join(artifacts, 'failure.json'),
          JSON.stringify(
            { test: name, state: await state(), targetEvents: events(), appErrors: app.errors },
            null,
            2
          )
        )
        throw error
      }
    })
  }
  try {
    await exec(
      'swiftc',
      [join(directory, 'paste_target.swift'), '-o', join(temporary, 'paste-target')],
      { timeout: 60_000 }
    )
    target = launch(join(temporary, 'paste-target'), [])
    await until('paste target startup', () => target.output.includes('"event":"ready"'))
    fixture = launch(process.execPath, [join(directory, 'fixture.mjs')], {
      ...process.env,
      UC_GPUI_FIXTURE_PORT: '0',
      UC_GPUI_IMAGE_FIXTURES: '1',
      UC_GPUI_FILTER_FIXTURES: '1',
      UC_GPUI_FIXTURE_THEME: process.env.UC_GPUI_E2E_THEME ?? 'light',
    })
    address = await until(
      'fixture startup',
      () => fixture.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0]
    )

    await run(
      'Tab from an empty search cycles the type filter, and Shift+Tab goes back',
      async () => {
        await key('tab')
        const cycled = await query({ query: '', types: ['text'] })
        await noPaste()
        await hotkey('shift,tab')
        await query({ query: '' }, cycled.searches)
        await noPaste()
      }
    )
    await run('typing a matching label still searches the full text', async () => {
      await text('工作 设计')
      await query({ query: '工作 设计' })
      await noPaste()
    })
    await run(
      'copying selected query text uses the editor rather than restoring history',
      async () => {
        await text('工作 设计')
        await query({ query: '工作 设计' })
        await hotkey('cmd,a')
        await hotkey('cmd,c')
        const beforeClipboard = events().length
        target.stdin.write('clipboard\n')
        const clipboard = await until('copied query text', () =>
          events()
            .slice(beforeClipboard)
            .find(e => e.event === 'clipboard')
        )
        assert.equal(clipboard.sha256, digest('工作 设计'))
        await noPaste()
      }
    )
    await run('Tab turns a matching word into a filter and keeps the remaining query', async () => {
      await text('工作 设计')
      await query({ query: '工作 设计' })
      await key('tab')
      await query({ query: '设计', tags: ['工作'] })
      await noPaste()
      await text('图片 设计')
      await query({ query: '图片 设计', tags: ['工作'] })
      await key('tab')
      await query({ query: '设计', types: ['image'], tags: ['工作'] })
      await noPaste()
    })
    await run('Escape dismisses suggestions without discarding the query', async () => {
      await text('工作 设计')
      await query({ query: '工作 设计' })
      await key('escape')
      await input('type', ['x'])
      await query({ query: '工作 设计x' })
      await noPaste()
    })
    // "#" lists every tag; the first one is `link`. Each flow starts from a fresh panel.
    const firstOfHash = async keys => {
      await text('#')
      await query({ query: '' })
      for (const name of keys) await key(name)
      await key('tab')
      const sent = await until('a search with one tag', async () => {
        const last = (await state()).lastSearch
        return last?.query === '' && last.tags.length === 1 && last
      })
      await noPaste()
      return sent.tags[0]
    }
    await run('Up and Down choose among suggestions: Tab alone takes the first', async () => {
      assert.equal(await firstOfHash([]), 'link')
    })
    await run('Up and Down choose among suggestions: Down moves to the next one', async () => {
      assert.notEqual(await firstOfHash(['down']), 'link')
    })
    await run('Up and Down choose among suggestions: Up after Down comes back', async () => {
      assert.equal(await firstOfHash(['down', 'up']), 'link')
    })
    // Plain words list three suggestions (image type, favorited, 工作) above the results. Up at the
    // first result enters the list from below; Tab or Enter accepts the highlighted one.
    await run('Up at the first result enters the suggestions from below', async () => {
      await text('图片 收藏 工作 预览')
      await query({ query: '图片 收藏 工作 预览' })
      await key('up')
      await key('tab')
      await query({ query: '图片 收藏 预览', tags: ['工作'] })
      await noPaste()
    })
    await run('Up twice reaches the second suggestion and Enter accepts it', async () => {
      await text('图片 收藏 工作 预览')
      await query({ query: '图片 收藏 工作 预览' })
      await key('up')
      await key('up')
      await key('return')
      await query({ query: '图片 工作 预览', tags: ['favorited'] })
      await noPaste()
    })
    await run('Down from the end of the suggestions returns to the results', async () => {
      await text('图片 收藏 工作 预览')
      await query({ query: '图片 收藏 工作 预览' })
      await key('up')
      await key('down')
      await key('return')
      await until('the selected result was pasted', async () => (await state()).restores.length > restoreBaseline)
    })
    await run('Backspace with nothing to delete does not search again', async () => {
      const before = (await state()).searches
      await key('delete')
      await key('delete')
      await key('delete')
      await delay(700)
      assert.equal((await state()).searches, before, 'The list must not refresh')
    })
    await run('Backspace in an empty search box removes the last filter chip', async () => {
      await text('#')
      await query({ query: '' })
      await key('tab')
      await until('one tag chip', async () => (await state()).lastSearch?.tags?.length === 1)
      await key('delete')
      await until('the chip is gone', async () => {
        const last = (await state()).lastSearch
        return last?.query === '' && last.tags.length === 0
      })
      await noPaste()
    })
    await run(
      'Backspace with text in the search box deletes a character, not the chip',
      async () => {
        await text('#')
        await query({ query: '' })
        await key('tab')
        await until('one tag chip', async () => (await state()).lastSearch?.tags?.length === 1)
        await input('type', ['ab'])
        await until('ab typed', async () => (await state()).lastSearch?.query === 'ab')
        await key('delete')
        await until('one character left', async () => {
          const last = (await state()).lastSearch
          return last?.query === 'a' && last.tags.length === 1
        })
        await noPaste()
      }
    )
    await run(
      'clicking a filter chip removes only that condition and returns input focus',
      async () => {
        await text('工作 设计')
        await query({ query: '工作 设计' })
        await key('tab')
        await query({ query: '设计', tags: ['工作'] })
        // The chip sits after the search icon in the 48 px search row.
        await clickAt(60, 25)
        await query({ query: '设计' })
        await input('type', ['x'])
        await query({ query: '设计x' })
        await noPaste()
      }
    )
    await run('pending search blocks pasting stale history results', async () => {
      await text('slow')
      await until('slow search started', async () => (await state()).searchStarts.includes('slow'))
      await key('return')
      await noPaste()
      await query({ query: 'slow' })
    })
    await run('several tags are alternatives sent in one request', async () => {
      await text('图片 收藏 工作 预览')
      await query({ query: '图片 收藏 工作 预览' })
      await key('tab')
      await query({ query: '收藏 工作 预览', types: ['image'] })
      await key('tab')
      await query({ query: '工作 预览', types: ['image'], tags: ['favorited'] })
      await key('tab')
      const sent = await query({ query: '预览', types: ['image'], tags: ['favorited', '工作'] })
      await noPaste()
      // The panel asks once and shows what the daemon returned: no paging to narrow the result.
      assert.ok(
        (await state()).requests.every(request => request.offset === 0),
        'The panel must not page through results'
      )
      const expected = (
        await (
          await fetch(
            `${address}/search/query?query=${encodeURIComponent('预览')}&contentTypes=image&tags=${encodeURIComponent('favorited,工作')}&limit=50&offset=0`,
            {
              headers: { authorization: 'Session fixture-session' },
            }
          )
        ).json()
      ).data.items
      assert.ok(expected.length > 1, 'The fixture must have more than the entry carrying both tags')
      await hotkey('cmd,c')
      await until(
        'first result copied',
        async () => (await state()).restores.length > restoreBaseline
      )
      assert.deepEqual((await state()).restores.slice(restoreBaseline), [expected[0].entryId])
      assert.ok(sent.searches > 0)
    })
    await run('@ picks a device and / picks a type', async () => {
      await text('@iph')
      await query({ query: '@iph' })
      await key('tab')
      await query({ query: '', sources: ['peer-iphone'] })
      await text('/image')
      await query({ query: '/image', sources: ['peer-iphone'] })
      await key('tab')
      await query({ query: '', types: ['image'], sources: ['peer-iphone'] })
      await noPaste()
    })
    await run(
      'a date in the search text becomes a time range and the next one replaces it',
      async () => {
        const hour = 3_600_000
        await text('docker 3d')
        await query({ query: 'docker 3d' })
        await key('tab')
        const three = await query({
          query: 'docker',
          time: r => Math.abs(r.toMs - r.fromMs + 1 - 72 * hour) <= hour,
        })
        await noPaste()
        await text('docker 昨天')
        await query({ query: 'docker 昨天', time: r => r.fromMs === three.lastSearch.fromMs })
        await key('tab')
        await query({
          query: 'docker',
          time: r => Math.abs(r.toMs - r.fromMs + 1 - 24 * hour) <= hour && r.toMs < Date.now(),
        })
        await noPaste()
      }
    )
    await run(
      'pinyin initials suggest a type and a time, and a second one replaces the first',
      async () => {
        const hour = 3_600_000
        // zt is yesterday; no fixture tag abbreviates to it, unlike jt (the tag 截图).
        await text('tp zt')
        await query({ query: 'tp zt' })
        await key('tab')
        await query({ query: 'zt', types: ['image'] })
        await key('tab')
        const yesterday = await query({
          query: '',
          types: ['image'],
          time: r => Math.abs(r.toMs - r.fromMs + 1 - 24 * hour) <= hour && r.toMs < Date.now(),
        })
        await text('bz')
        await query({
          query: 'bz',
          types: ['image'],
          time: r => r.fromMs === yesterday.lastSearch.fromMs,
        })
        await key('tab')
        await query({
          query: '',
          types: ['image'],
          time: r => Math.abs(r.toMs - r.fromMs + 1 - 168 * hour) <= hour,
        })
        await noPaste()
      }
    )
    await run(
      'plain Enter pastes the history result even when suggestions are visible',
      async () => {
        await text('工作 设计')
        await query({ query: '工作 设计' })
        await delay(120)
        await key('return')
        await until(
          'history entry restored',
          async () => (await state()).restores.length === restoreBaseline + 1
        )
        assert.deepEqual((await state()).restores.slice(restoreBaseline), ['fixture-0'])
        const expected = digest('GPUI quick panel paste verification · 工作 设计')
        await until('text pasted into isolated target', () =>
          events()
            .filter(e => e.event === 'text')
            .slice(textBaseline)
            .some(e => e.sha256 === expected)
        )
      }
    )
    await run('Command+digit pastes the row that carries that digit', async () => {
      const rows = await defaultRows()
      // Wait until the panel has drawn the same list, so the digit maps to what was read.
      await delay(300)
      await hotkey('cmd,2')
      await until(
        'second row restored',
        async () => (await state()).restores.length === restoreBaseline + 1
      )
      assert.deepEqual((await state()).restores.slice(restoreBaseline), [rows[1].entryId])
    })
    await run(
      'Command+K opens the action list, and Enter runs the row under its cursor',
      async () => {
        const rows = await defaultRows()
        await delay(300)
        await hotkey('cmd,k')
        await delay(600)
        // Evidence for the eye: the history window and the list in the satellite window.
        for (const [title, name] of [
          ['UniClipboard History', 'actions-history'],
          ['UniClipboard Preview', 'actions-list'],
        ]) {
          await exec(peekaboo, [
            'image',
            '--pid',
            String(app.pid),
            '--window-title',
            title,
            '--path',
            join(artifacts, `${name}.png`),
            '--capture-focus',
            'background',
            '--no-remote',
          ]).catch(() => {})
        }
        // With the list open, Up moves its cursor from the first row to the last one (Settings)
        // and then to Delete above it.
        await key('up')
        await key('up')
        await key('return')
        await until(
          'selected entry deleted from the list',
          async () => (await state()).deleted.length === deletedBaseline + 1
        )
        assert.deepEqual((await state()).deleted.slice(deletedBaseline), [rows[0].entryId])
        assert.equal(
          (await state()).restores.length,
          restoreBaseline,
          'Enter in the list must not paste'
        )
      }
    )
    await run('Escape closes the action list and keeps the panel open', async () => {
      await delay(300)
      await hotkey('cmd,k')
      await delay(300)
      await key('escape')
      await delay(200)
      assert.equal(app.exitCode, null)
      await input('type', ['x'])
      await query({ query: 'x' })
      assert.deepEqual((await state()).deleted.slice(deletedBaseline), [])
    })
    await run('Command+Backspace clears the search text and the filters', async () => {
      await text('工作 设计')
      await query({ query: '工作 设计' })
      await key('tab')
      const filtered = await query({ query: '设计', tags: ['工作'] })
      await hotkey('cmd,delete')
      await query({ query: '' }, filtered.searches)
      await noPaste()
    })
    await run('Command+Enter pastes and keeps the panel open', async () => {
      const rows = await defaultRows()
      await delay(300)
      await hotkey('cmd,return')
      await until(
        'entry restored',
        async () => (await state()).restores.length === restoreBaseline + 1
      )
      assert.deepEqual((await state()).restores.slice(restoreBaseline), [rows[0].entryId])
      await delay(300)
      await input('type', ['x'])
      await query({ query: 'x' })
    })
    await run(
      'the image grid moves in two dimensions and Enter pastes the selected cell',
      async () => {
        const images = (
          await (
            await fetch(`${address}/search/query?query=&contentTypes=image&limit=50&offset=0`, {
              headers: { authorization: 'Session fixture-session' },
            })
          ).json()
        ).data.items
        assert.equal(images.length, 12)
        await text('/image')
        await query({ query: '/image' })
        await key('tab')
        await query({ query: '', types: ['image'] })
        await delay(1500)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'grid-history.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ]).catch(() => {})
        // From cell 1: right to cell 2, down to cell 5.
        await key('right')
        await key('down')
        await delay(300)
        await key('return')
        await until(
          'cell 5 restored',
          async () => (await state()).restores.length === restoreBaseline + 1
        )
        assert.deepEqual((await state()).restores.slice(restoreBaseline), [images[4].entryId])
      }
    )
    await run(
      'the image grid scrolls by rows and Command plus a digit pastes that cell',
      async () => {
        const images = (
          await (
            await fetch(`${address}/search/query?query=&contentTypes=image&limit=50&offset=0`, {
              headers: { authorization: 'Session fixture-session' },
            })
          ).json()
        ).data.items
        await text('/image')
        await query({ query: '/image' })
        await key('tab')
        await query({ query: '', types: ['image'] })
        await delay(1000)
        // Three rows down leaves the first page; the badges then name entries 4 to 12.
        await key('down')
        await key('down')
        await key('down')
        await delay(400)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'grid-scrolled.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ]).catch(() => {})
        await hotkey('cmd,9')
        await until(
          'cell 9 restored',
          async () => (await state()).restores.length === restoreBaseline + 1
        )
        assert.deepEqual((await state()).restores.slice(restoreBaseline), [images[11].entryId])
      }
    )
    await run('the preview presents code, a link and files each in their own form', async () => {
      // Screenshots are the evidence; the assertion is that the satellite window exists for each.
      for (const [word, name] of [
        ['kind_code', 'code'],
        ['github.com', 'link'],
        ['kind_file', 'file'],
      ]) {
        await text(word)
        await query({ query: word })
        await delay(1200)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard Preview',
          '--path',
          join(artifacts, `preview-${name}.png`),
          '--capture-focus',
          'background',
          '--no-remote',
        ])
        await stat(join(artifacts, `preview-${name}.png`))
      }
    })
    await run('one letter the index cannot search is "nothing found", not a failed search', async () => {
      await text('l')
      await query({ query: 'l' })
      await delay(500)
      assert.ok(
        !app.errors.includes('Quick panel query failed'),
        'The panel must not treat an unsearchable query as a failed search'
      )
      await noPaste()
    })
    await run(
      'a search with nothing found offers to loosen the condition with the most effect',
      async () => {
        await text('#工作 预览')
        await query({ query: '#工作 预览' })
        await key('tab')
        await query({ query: '预览', tags: ['工作'] })
        // With no suggestion left, Tab cycles the type filter; text has no 工作 image.
        await key('tab')
        const empty = await query({ query: '预览', types: ['text'], tags: ['工作'] })
        await until('counts for the relaxations requested', async () =>
          (await state()).requests.some(
            r => r.limit === 1 && r.types.length === 0 && r.tags.includes('工作')
          )
        )
        await delay(300)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'state-no-match.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ])
        await key('return')
        await query({ query: '预览', tags: ['工作'] }, empty.searches)
        await noPaste()
      }
    )
    await run(
      'a daemon that does not answer shows the disconnected page and is retried',
      async () => {
        await text('drop')
        await until(
          'first attempt',
          async () => (await state()).searchStarts.filter(q => q === 'drop').length >= 1
        )
        await delay(600)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'state-disconnected.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ])
        const attempts = () =>
          state().then(value => value.searchStarts.filter(q => q === 'drop').length)
        await until('automatic retry', async () => (await attempts()) >= 2, 8000)
        const before = await attempts()
        await key('return')
        await until('immediate retry on Enter', async () => (await attempts()) > before, 2000)
        await hotkey('cmd,delete')
        await query({ query: '' }, (await state()).searches - 1)
        await noPaste()
      }
    )
    await run('an empty history shows the first-use page with the shortcut', async () => {
      await fetch(`${address}/__test/empty?on=1`)
      try {
        const before = await state()
        await exec(peekaboo, ['hotkey', '--keys', 'ctrl,alt,space', '--no-remote'], {
          timeout: 10_000,
        })
        await delay(300)
        await exec(peekaboo, ['hotkey', '--keys', 'ctrl,alt,space', '--no-remote'], {
          timeout: 10_000,
        })
        await until(
          'panel searched the empty history',
          async () => (await state()).searches > before.searches
        )
        await delay(400)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'state-first-use.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ])
      } finally {
        await fetch(`${address}/__test/empty?on=0`)
      }
    })
    // The requests the panel makes of the GUI arrive as JSON lines on its stdout.
    const requests = () =>
      app.output
        .split('\n')
        .filter(line => line.includes('"request"'))
        .map(line => JSON.parse(line).request)
    await run(
      'Command+Shift+O and Command+comma ask the GUI for the main window and the settings',
      async () => {
        await delay(300)
        await hotkey('cmd,shift,o')
        await until('main window requested', async () => requests().includes('show_main_window'))
        // The request closes the panel; open it again for the next one.
        await delay(300)
        await exec(peekaboo, ['hotkey', '--keys', 'ctrl,alt,space', '--no-remote'], {
          timeout: 10_000,
        })
        await delay(500)
        await hotkey('cmd,comma')
        await until('settings requested', async () => requests().includes('open_settings'))
        assert.deepEqual(requests(), ['show_main_window', 'open_settings'])
      }
    )
    await run('the last row of the action list opens the settings', async () => {
      await delay(300)
      await hotkey('cmd,k')
      await delay(300)
      await key('up')
      await key('return')
      await until('settings requested from the list', async () =>
        requests().includes('open_settings')
      )
    })
    await run(
      'a locked history shows the lock page, and Enter asks the GUI for the main window',
      async () => {
        await text('locked')
        await until('locked search', async () => (await state()).searchStarts.includes('locked'))
        await delay(500)
        await exec(peekaboo, [
          'image',
          '--pid',
          String(app.pid),
          '--window-title',
          'UniClipboard History',
          '--path',
          join(artifacts, 'state-locked.png'),
          '--capture-focus',
          'background',
          '--no-remote',
        ])
        await key('return')
        await until('main window requested to unlock', async () =>
          requests().includes('show_main_window')
        )
        // The panel looks again by itself while it stays locked.
        assert.equal((await state()).restores.length, restoreBaseline)
      }
    )
    await run('Command+Q does not quit the panel', async () => {
      await hotkey('cmd,q')
      await delay(1000)
      assert.equal(
        app.exitCode,
        null,
        'A supervised panel must keep running; its GUI decides when it stops'
      )
      assert.equal(app.signalCode, null)
    })
    await run(
      'Option+Backspace edits the search text and Command+Shift+Backspace deletes the entry',
      async () => {
        // ASCII on purpose: the input deletes Chinese text one character at a time.
        await text('alpha beta')
        await query({ query: 'alpha beta' })
        await hotkey('alt,delete')
        await query({ query: 'alpha' })
        await delay(400)
        assert.deepEqual(
          (await state()).deleted.slice(deletedBaseline),
          [],
          'Deleting a word in the search box must not delete an entry'
        )
        await key('escape')
        const cleared = await query({ query: '' })
        const rows = await defaultRows()
        await delay(300)
        await hotkey('cmd,shift,delete')
        await until(
          'selected entry deleted',
          async () => (await state()).deleted.length === deletedBaseline + 1
        )
        assert.deepEqual((await state()).deleted.slice(deletedBaseline), [rows[0].entryId])
        assert.ok(cleared.searches >= 1)
      }
    )
  } finally {
    await stop(app)
    await stop(fixture)
    if (target) {
      target.stdin.end('quit\n')
      await until(
        'paste target exits and restores clipboard',
        () => target.exitCode !== null || target.signalCode !== null,
        3000
      ).catch(() => stop(target))
    }
    const failed = await access(join(artifacts, 'failure.json')).then(
      () => true,
      () => false
    )
    if (failed) t.diagnostic(`Failure artifacts: ${artifacts}`)
    if (artifacts !== temporary || !failed) await rm(temporary, { recursive: true, force: true })
  }
})
