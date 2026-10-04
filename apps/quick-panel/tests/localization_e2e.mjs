// Native end-to-end check that the GPUI quick panel shows its text in the configured language.
//
//   UC_GPUI_L10N_E2E_CONFIRM=1 UC_GPUI_E2E_BINARY=<uniclip-quick-panel> \
//     node --test apps/quick-panel/tests/localization_e2e.mjs
//
// Black box: the real panel process against the synthetic daemon (`fixture.mjs`), opened and
// driven only through what is bound to its own process: `toggle` lines on its standard input
// (`UC_GPUI_TEST_CONTROL=stdin`, which also leaves the global shortcut and the modifier double tap
// unregistered) and key presses delivered to its pid. Text is read back from screenshots of its
// own windows with Vision OCR (`read_text.swift`).
//
// It never pastes, copies or restores: the fixture refuses restores without touching the system
// clipboard (`UC_GPUI_FIXTURE_NO_CLIPBOARD=1`), and the run fails if one was even attempted. The
// panel still appears on screen and takes focus for a few seconds per case, so it refuses to run
// unless `UC_GPUI_L10N_E2E_CONFIRM=1` says that has been arranged.
import assert from 'node:assert/strict'
import { spawn, execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdtemp, mkdir, readFile, stat, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve, dirname } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'

const exec = promisify(execFile)
const directory = dirname(fileURLToPath(import.meta.url))
const peekaboo = process.env.PEEKABOO_BIN ?? '/opt/homebrew/bin/peekaboo'
const delay = ms => new Promise(done => setTimeout(done, ms))

// What each language must show, as fragments that survive OCR (no punctuation or spacing).
const EXPECTED = {
  'zh-CN': {
    ocr: 'zh-Hans',
    search: '搜索',
    all: '全部',
    actions: '操作',
    plain: '粘贴为纯文本',
    hint: '选择',
    locked: '应用界面已锁定',
    firstUse: '还没有剪贴板历史',
    chip: '9月1日',
  },
  'zh-TW': {
    ocr: 'zh-Hant',
    search: '搜尋',
    all: '全部',
    actions: '操作',
    plain: '貼上為純文字',
    hint: '選擇',
    locked: '應用程式介面已鎖定',
    firstUse: '還沒有剪貼簿歷史',
    chip: '9月1日',
  },
  'en-US': {
    ocr: 'en-US',
    search: 'Search',
    all: 'All',
    actions: 'Actions',
    plain: 'Paste as plain text',
    hint: 'select',
    locked: 'The app is locked',
    firstUse: 'No clipboard history yet',
    chip: 'Sep 1',
  },
  'ja-JP': {
    ocr: 'ja-JP',
    search: '検索',
    all: 'すべて',
    actions: '操作',
    plain: 'プレーンテキストで貼り付け',
    hint: '選択',
    locked: 'アプリはロックされています',
    firstUse: 'クリップボード履歴はまだありません',
    chip: '9月1日',
  },
  'ru-RU': {
    ocr: 'ru-RU',
    search: 'Поиск',
    all: 'Все',
    actions: 'Действия',
    plain: 'Вставить как обычный текст',
    hint: 'выбор',
    locked: 'Приложение заблокировано',
    firstUse: 'История буфера обмена пока пуста',
    chip: '01.09',
  },
  'pt-BR': {
    ocr: 'pt-BR',
    search: 'Pesquise',
    all: 'Todos',
    actions: 'Ações',
    plain: 'Colar como texto simples',
    hint: 'selecionar',
    locked: 'O app está bloqueado',
    firstUse: 'Ainda não há histórico',
    chip: '01/09',
  },
}
// Labels of the old, hard-coded panel. None may appear in a language other than Chinese.
const CHINESE_LABELS = ['粘贴为纯文本', '粘贴并保持面板', '只复制', '搜索，或输入', '全部']

const squeeze = text => text.replace(/\s+/g, '').toLowerCase()
const contains = (lines, fragment) => squeeze(lines.join('\n')).includes(squeeze(fragment))

// The bundle the main window would pick for a language tag (`normalize_language`).
function bundleFor(tag) {
  const [primary, ...rest] = tag.toLowerCase().split(/[-_]/)
  if (primary === 'zh')
    return rest.some(s => ['hant', 'tw', 'hk', 'mo'].includes(s)) ? 'zh-TW' : 'zh-CN'
  return { ja: 'ja-JP', ru: 'ru-RU', pt: 'pt-BR' }[primary] ?? 'en-US'
}

async function systemLanguage() {
  const { stdout } = await exec('defaults', ['read', '-g', 'AppleLanguages'])
  return stdout.match(/"?([A-Za-z]{2,3}(?:[-_][A-Za-z0-9]+)*)"?/)?.[1] ?? ''
}

function launch(command, args, env) {
  const child = spawn(command, args, { env, stdio: ['pipe', 'pipe', 'pipe'] })
  child.output = ''
  child.errors = ''
  child.stdout.setEncoding('utf8').on('data', data => (child.output += data))
  child.stderr.setEncoding('utf8').on('data', data => (child.errors += data))
  return child
}

// Stops a process this test started, by its own pid only.
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return
  const exited = new Promise(done => child.once('exit', done))
  child.stdin.end()
  await Promise.race([exited, delay(1500)])
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGTERM')
    await Promise.race([exited, delay(1500)])
  }
}

async function until(description, predicate, timeout = 10_000) {
  const end = Date.now() + timeout
  while (Date.now() < end) {
    const result = await predicate()
    if (result) return result
    await delay(80)
  }
  throw new Error(`Timed out: ${description}`)
}

test('GPUI quick panel follows the configured language', { timeout: 600_000 }, async t => {
  assert.equal(process.platform, 'darwin', 'Native E2E requires macOS')
  assert.equal(
    process.env.UC_GPUI_L10N_E2E_CONFIRM,
    '1',
    'The panel takes focus on screen; set UC_GPUI_L10N_E2E_CONFIRM=1 once that is arranged'
  )
  const binary = resolve(process.env.UC_GPUI_E2E_BINARY ?? '')
  assert.ok(process.env.UC_GPUI_E2E_BINARY, 'UC_GPUI_E2E_BINARY must name the binary under test')
  const artifacts = resolve(
    process.env.UC_GPUI_E2E_ARTIFACTS ?? (await mkdtemp(join(tmpdir(), 'uc-gpui-l10n-')))
  )
  await mkdir(artifacts, { recursive: true })
  const ocrBinary = join(artifacts, 'read-text')
  await exec('swiftc', [join(directory, 'read_text.swift'), '-o', ocrBinary], {
    timeout: 120_000,
  })
  const system = await systemLanguage()
  const manifest = {
    commit: (await exec('git', ['rev-parse', 'HEAD'], { cwd: directory })).stdout.trim(),
    dirty: (await exec('git', ['status', '--porcelain'], { cwd: directory })).stdout.trim() !== '',
    binary,
    binaryBuilt: (await stat(binary)).mtime.toISOString(),
    binarySha256: createHash('sha256')
      .update(await readFile(binary))
      .digest('hex'),
    systemLanguage: system,
    steps: [],
  }
  t.diagnostic(`Artifacts: ${artifacts}`)

  let fixture, app, address
  const fixtureState = async () => (await fetch(`${address}/__test/state`)).json()
  const startFixture = async language => {
    fixture = launch(process.execPath, [join(directory, 'fixture.mjs')], {
      ...process.env,
      UC_GPUI_FIXTURE_PORT: '0',
      UC_GPUI_FIXTURE_THEME: 'light',
      UC_GPUI_FIXTURE_LANGUAGE: language,
      UC_GPUI_FIXTURE_NO_CLIPBOARD: '1',
    })
    address = await until(
      'fixture startup',
      () => fixture.output.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0]
    )
  }
  const startPanel = async () => {
    const env = {
      ...process.env,
      UC_PROFILE: `l10n-e2e-${process.pid}`,
      UNICLIPBOARD_DAEMON_BASE_URL: address,
      UNICLIPBOARD_DAEMON_TOKEN_PATH: join(directory, 'fixture-token.txt'),
      UC_GPUI_TEST_CONTROL: 'stdin',
      UC_GPUI_SCALE: '1',
    }
    delete env.UC_GPUI_SHORTCUT
    app = launch(binary, [], env)
    await until('settings read at startup', async () => (await fixtureState()).settingsReads > 0)
  }
  // Opens or closes the panel; opening reads the settings again, which is how a show is noticed.
  const toggle = async () => {
    const before = (await fixtureState()).settingsReads
    app.stdin.write('toggle\n')
    return before
  }
  const open = async () => {
    const before = await toggle()
    await until('panel opened', async () => (await fixtureState()).settingsReads > before)
    await delay(700)
  }
  const press = async keys => {
    assert.equal(app.exitCode, null, `panel exited: ${app.errors}`)
    await exec(
      peekaboo,
      ['hotkey', '--keys', keys, '--pid', String(app.pid), '--no-auto-focus', '--no-remote'],
      { timeout: 10_000 }
    )
    await delay(500)
  }
  const type = async text => {
    await exec(
      peekaboo,
      ['type', text, '--pid', String(app.pid), '--no-auto-focus', '--no-remote'],
      { timeout: 10_000 }
    )
    await delay(600)
  }
  const windows = async () => {
    const { stdout } = await exec(peekaboo, [
      'list',
      'windows',
      '--pid',
      String(app.pid),
      '--no-remote',
      '--json',
    ])
    return JSON.parse(stdout).data.windows
  }
  // Screenshots one of the panel's own windows and reads its text.
  const read = async (step, which, language) => {
    const all = await windows()
    const window =
      which === 'history'
        ? all.find(w => w.title === 'UniClipboard History')
        : all.find(w => w.title !== 'UniClipboard History')
    assert.ok(window, `${which} window of pid ${app.pid} not found`)
    const image = join(artifacts, `${step}-${which}.png`)
    await exec('/usr/sbin/screencapture', ['-x', '-o', '-l', String(window.window_id), image])
    const ocr = async languages =>
      JSON.parse((await exec(ocrBinary, [image, languages])).stdout).lines
    const lines = await ocr(EXPECTED[language].ocr)
    // A second pass in Chinese, since a recognizer set to another language cannot see Chinese.
    const chinese = language.startsWith('zh') ? lines : await ocr('zh-Hans')
    manifest.steps.push({
      step,
      which,
      language,
      image,
      windowId: window.window_id,
      lines,
      chinese,
    })
    return Object.assign(lines, { chinese })
  }
  const finish = async () => {
    await stop(app)
    app = null
    const state = await fixtureState()
    // Nothing may have tried to restore an entry, which is the only way to the clipboard.
    assert.equal(state.restores.length, 0, 'no restore may happen')
    assert.equal(state.refusedRestores ?? 0, 0, 'no restore may even be attempted')
    await stop(fixture)
    fixture = null
  }

  const checkHistoryAndMenu = async (step, language) => {
    const e = EXPECTED[language]
    const history = await read(step, 'history', language)
    assert.ok(contains(history, e.search), `${step}: placeholder ${e.search} in ${history}`)
    assert.ok(contains(history, e.all), `${step}: filter ${e.all}`)
    assert.ok(contains(history, e.actions), `${step}: footer ${e.actions}`)
    await press('cmd,k')
    const menu = await read(step, 'menu', language)
    assert.ok(contains(menu, e.actions), `${step}: menu title ${e.actions} in ${menu}`)
    assert.ok(contains(menu, e.plain), `${step}: menu item ${e.plain}`)
    assert.ok(contains(menu, e.hint), `${step}: hint ${e.hint}`)
    if (!language.startsWith('zh'))
      for (const label of CHINESE_LABELS)
        assert.ok(
          !contains([...history.chinese, ...menu.chinese], label),
          `${step}: old label ${label}`
        )
  }

  const run = async (name, language, body) => {
    await t.test(name, async () => {
      await startFixture(language)
      try {
        await startPanel()
        await body()
      } finally {
        await writeFile(join(artifacts, 'manifest.json'), JSON.stringify(manifest, null, 2))
        await finish()
      }
    })
  }

  try {
    // a, b, h: every shipped language as the configured one.
    for (const language of Object.keys(EXPECTED))
      await run(`configured ${language}`, language, async () => {
        await open()
        await checkHistoryAndMenu(`configured-${language}`, language)
      })
    // h: a tag without a bundle falls back to English, as in the main window.
    await run('invalid tag falls back to English', 'xx-INVALID', async () => {
      await open()
      await checkHistoryAndMenu('invalid-tag', 'en-US')
    })
    // c: no configured language follows the system language.
    await run('unset language follows the system', '', async () => {
      await open()
      await checkHistoryAndMenu('unset', bundleFor(system))
    })
    // d: a change while the panel runs shows on the next open. The first frame is recorded, not
    // asserted: it may still be the previous language until the settings arrive.
    await run('runtime switch applies on the next open', 'en-US', async () => {
      await open()
      await checkHistoryAndMenu('switch-before', 'en-US')
      await fetch(`${address}/__test/language?value=zh-CN`)
      await toggle()
      await delay(800)
      const before = await toggle()
      await read('switch-first-frame', 'history', 'zh-CN')
      await until('settings read again', async () => (await fixtureState()).settingsReads > before)
      await delay(700)
      await checkHistoryAndMenu('switch-after', 'zh-CN')
    })
    // e: the locked page follows the configured language, not the system one.
    const other = bundleFor(system) === 'ja-JP' ? 'ru-RU' : 'ja-JP'
    await run('locked page follows the configured language', other, async () => {
      await fetch(`${address}/__test/locked?on=1`)
      await open()
      const lines = await read('locked', 'history', other)
      assert.ok(contains(lines, EXPECTED[other].locked), `locked page in ${other}: ${lines}`)
    })
    // f: a typed date range becomes a chip labelled in the configured language.
    await run('date chip is localized', 'en-US', async () => {
      await open()
      await type('9.1-9.15')
      // Tab accepts the suggested time range. Never Enter: it would paste the selected entry.
      await press('tab')
      const lines = await read('date-chip', 'history', 'en-US')
      assert.ok(contains(lines, EXPECTED['en-US'].chip), `date chip: ${lines}`)
    })
    // g: a page the panel draws itself, here the first-use page.
    await run('first-use page is localized', 'pt-BR', async () => {
      await fetch(`${address}/__test/empty?on=1`)
      await open()
      const lines = await read('first-use', 'history', 'pt-BR')
      assert.ok(contains(lines, EXPECTED['pt-BR'].firstUse), `first-use page: ${lines}`)
    })
  } finally {
    await writeFile(join(artifacts, 'manifest.json'), JSON.stringify(manifest, null, 2))
    await stop(app)
    await stop(fixture)
  }
})
