import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
// Synthetic daemon for repeatable UI checks. Never reads real user history.
import { createServer } from 'node:http'

const rows = Array.from({ length: 125 }, (_, index) => ({
  entryId: `fixture-${index}`,
  contentType: 'text',
  activeTimeMs: Date.now() - index * 60000,
  tags: index === 0 ? ['favorited', '工作'] : [],
  textPreview:
    index === 0
      ? 'GPUI quick panel paste verification · 工作 设计'
      : index === 1
        ? '中文搜索验证：剪贴板历史'
        : `Clipboard sample ${index} - a long preview that remains on one line even when the window is narrow`,
  charCount: 80,
  mimeType: 'text/plain',
  fileExtensions: [],
  fileNames: [],
  filePaths: [],
  linkUrls: [],
  sourceDevice: null,
  payloadState: index === 2 ? 'Lost' : null,
}))
const state = {
  searches: 0,
  restores: [],
  deleted: [],
  authenticated: 0,
  lastSearch: null,
  requests: [],
  searchStarts: [],
  settingsReads: 0,
  tagsReads: 0,
}
const fullText = new Map([
  [
    'fixture-4',
    Array.from(
      { length: 120 },
      (_, i) => `第 ${i + 1} 行：这是一段用于验证预览高度上限与内部滚动的长文本。`
    ).join('\n'),
  ],
  [
    'fixture-5',
    '多行内容验证\n第二行：预览跟随记录\n第三行：小箭头保持对齐\n第四行：窗口高度由内容决定\n第五行：历史窗口保持不动',
  ],
])
// Two devices' worth of history: rows 6 and 7 came from the paired phone.
rows[6].sourceDevice = 'peer-iphone'
rows[7].sourceDevice = 'peer-iphone'
// One row of each kind the preview presents differently: code, a link and files.
Object.assign(rows[8], {
  tags: ['code'],
  textPreview: 'fn kind_code() {\n    let answer = 42;\n    println!("{answer}");\n}',
})
Object.assign(rows[9], {
  tags: ['link'],
  textPreview: 'https://github.com/uniclipboard/desktop/pull/1767',
  linkUrls: ['https://github.com/uniclipboard/desktop/pull/1767'],
})
Object.assign(rows[10], {
  contentType: 'file',
  textPreview: 'kind_file.pdf',
  fileNames: ['kind_file.pdf', 'kind_notes.txt'],
  filePaths: ['/Users/test/Documents/kind_file.pdf', '/Users/test/Documents/kind_notes.txt'],
  fileExtensions: ['pdf', 'txt'],
})
rows[4].textPreview = '长文本验证（120 行）'
rows[5].textPreview = '多行内容验证（5 行）'
const imageBytes = new Map()
const settings = JSON.parse(readFileSync(new URL('./settings.json', import.meta.url), 'utf8'))
settings.general.theme = process.env.UC_GPUI_FIXTURE_THEME ?? 'system'
// The panel reads words as pinyin initials only in a Chinese interface, whatever the system says.
// An empty value leaves the language unset, so the panel follows the system language.
settings.general.language = process.env.UC_GPUI_FIXTURE_LANGUAGE ?? 'zh-CN'
if (process.env.UC_GPUI_IMAGE_FIXTURES === '1') {
  const images = [
    ['landscape', '横图预览 · 设计'],
    ['portrait', '竖图预览'],
    ['transparent', '透明图片预览'],
    ['small', '小图预览'],
  ]
  const imageRows = images.map(([name, label]) => {
    const entryId = `image-${name}`
    imageBytes.set(entryId, readFileSync(new URL(`./images/${name}.png`, import.meta.url)))
    return {
      ...rows[0],
      entryId,
      textPreview: label,
      contentType: 'image',
      mimeType: 'image/png',
      tags: [
        'image',
        ...(name === 'landscape'
          ? ['favorited', '工作']
          : name === 'portrait'
            ? ['工作']
            : name === 'transparent'
              ? ['favorited']
              : []),
      ],
      payloadState: null,
      charCount: null,
    }
  })
  if (process.env.UC_GPUI_FILTER_FIXTURES === '1') imageRows.reverse()
  rows.unshift(...imageRows)
  // Eight more images at the end make twelve, so the 3 x 3 grid has a fourth row to scroll to.
  const kinds = images.map(([name]) => name)
  for (let n = 1; n <= 8; n++) {
    const entryId = `image-extra-${n}`
    imageBytes.set(
      entryId,
      readFileSync(new URL(`./images/${kinds[n % kinds.length]}.png`, import.meta.url))
    )
    rows.push({
      ...rows[0],
      entryId,
      textPreview: `网格图 ${n}`,
      contentType: 'image',
      mimeType: 'image/png',
      tags: ['image'],
      payloadState: null,
      charCount: null,
    })
  }
}
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost')
  const json = (status, body) => {
    response.writeHead(status, { 'content-type': 'application/json' })
    response.end(JSON.stringify(body))
  }
  if (url.pathname === '/__test/state') return json(200, state)
  // Changes the configured language, as the main window's language setting does; the panel
  // reads it again the next time it opens.
  if (url.pathname === '/__test/language') {
    settings.general.language = url.searchParams.get('value') || null
    return json(200, { language: settings.general.language })
  }
  // Empties or refills the history, to see the first-use page.
  if (url.pathname === '/__test/empty') {
    state.empty = url.searchParams.get('on') === '1'
    return json(200, { empty: state.empty })
  }
  if (url.pathname === '/auth/connect') {
    if (request.headers.authorization !== 'Bearer fixture-token') return json(401, {})
    state.authenticated++
    return json(200, {
      data: { sessionToken: 'fixture-session', expiresInSecs: 3600, refreshAtSecs: 3000 },
      ts: Date.now(),
    })
  }
  if (request.headers.authorization !== 'Session fixture-session') return json(401, {})
  if (url.pathname === '/settings') {
    state.settingsReads++
    return json(200, { data: settings, ts: Date.now() })
  }
  if (url.pathname === '/search/tags') {
    state.tagsReads++
    const builtins = ['link', 'code', 'favorited', 'image', 'directory']
    return json(200, {
      data: [
        ...builtins,
        '工作',
        '设计素材与灵感收集',
        '项目资料归档',
        '稍后处理',
        '待整理',
        '参考资料',
        '生活',
        '阅读',
        '研究',
        '截图',
        '临时备忘',
      ].map(tagId => ({
        tagId,
        count: rows.filter(row => row.tags.includes(tagId)).length,
        isBuiltin: builtins.includes(tagId),
      })),
      ts: Date.now(),
    })
  }
  if (url.pathname === '/paired-devices')
    return json(200, {
      data: [
        {
          peerId: 'peer-iphone',
          deviceName: 'iPhone',
          pairingState: 'paired',
          lastSeenAtMs: null,
          connected: true,
          channel: 'direct',
          connectionAddress: null,
        },
      ],
      ts: Date.now(),
    })
  if (url.pathname === '/search/query') {
    // A request for one entry only counts matches (for the suggestions to loosen a search); it is
    // recorded in `requests` but is not a search the panel is showing.
    const countOnly = url.searchParams.get('limit') === '1'
    if (!countOnly) state.searches++
    const query = url.searchParams.get('query') ?? ''
    if (!countOnly) state.searchStarts.push(query)
    // A dropped connection, as when the daemon is down.
    if (query === 'drop') return request.socket.destroy()
    // The content lock, as the daemon answers while history is locked.
    if (query === 'locked')
      return json(423, { error: { code: 'session_locked', message: 'Locked' } })
    if (query === 'error')
      return json(503, { error: { code: 'index_rebuilding', message: 'Synthetic failure' } })
    // Text the index has no term for, such as one Latin letter: the daemon refuses it as invalid.
    if (query === 'l')
      return json(400, {
        error: { code: 'invalid_query', message: 'query produced no searchable terms' },
      })
    if (query === 'slow') await new Promise(resolve => setTimeout(resolve, 900))
    // Every list parameter is comma separated, and the values of one parameter are alternatives.
    const list = name => (url.searchParams.get(name) ?? '').split(',').filter(Boolean)
    const types = list('contentTypes'),
      tags = list('tags'),
      sources = list('sourceDevices')
    const fromMs = url.searchParams.has('fromMs') ? Number(url.searchParams.get('fromMs')) : null
    const toMs = url.searchParams.has('toMs') ? Number(url.searchParams.get('toMs')) : null
    const offset = Number(url.searchParams.get('offset') ?? 0),
      limit = Number(url.searchParams.get('limit') ?? 50)
    const entry = { query, types, tags, sources, fromMs, toMs, offset, limit }
    if (!countOnly) state.lastSearch = entry
    state.requests.push(entry)
    const results = state.empty
      ? []
      : rows.filter(
          row =>
            row.textPreview.toLowerCase().includes(query.toLowerCase()) &&
            (!types.length || types.includes(row.contentType)) &&
            (!tags.length || tags.some(tag => row.tags.includes(tag))) &&
            (!sources.length || sources.includes(row.sourceDevice)) &&
            (fromMs === null || (row.activeTimeMs >= fromMs && row.activeTimeMs <= toMs))
        )
    return json(200, {
      data: {
        items: results.slice(offset, offset + limit),
        total: results.length,
        hasMore: results.length > offset + limit,
        state: 'ready',
      },
      ts: Date.now(),
    })
  }
  if (request.method === 'GET' && url.pathname.startsWith('/clipboard/entries/')) {
    if (url.pathname.endsWith('/resource')) {
      const bytes = imageBytes.get(url.pathname.split('/').at(-2))
      if (!bytes) return json(404, {})
      return json(200, {
        data: {
          blobId: null,
          mimeType: 'image/png',
          sizeBytes: bytes.length,
          url: null,
          inlineData: bytes.toString('base64'),
        },
        ts: Date.now(),
      })
    }
    const row = rows.find(row => row.entryId === url.pathname.split('/').at(-1))
    if (!row) return json(404, {})
    const content = fullText.get(row.entryId) ?? row.textPreview
    return json(200, {
      data: {
        id: row.entryId,
        content,
        sizeBytes: Buffer.byteLength(content),
        createdAtMs: row.activeTimeMs,
        activeTimeMs: row.activeTimeMs,
        mimeType: row.mimeType,
      },
      ts: Date.now(),
    })
  }
  if (request.method === 'POST' && url.pathname.startsWith('/clipboard/restore/')) {
    const row = rows.find(row => row.entryId === url.pathname.split('/').at(-1))
    if (!row || row.payloadState === 'Lost') return json(404, {})
    const result = spawnSync('pbcopy', { input: row.textPreview })
    if (result.status !== 0) return json(500, {})
    state.restores.push(row.entryId)
    response.writeHead(204)
    return response.end()
  }
  if (request.method === 'DELETE' && url.pathname.startsWith('/clipboard/entries/')) {
    // Records the request only; the rows stay so that every test starts from the same list.
    state.deleted.push(url.pathname.split('/').at(-1))
    response.writeHead(204)
    return response.end()
  }
  json(404, {})
})
server.listen(Number(process.env.UC_GPUI_FIXTURE_PORT ?? 48173), '127.0.0.1', () =>
  console.log(`Synthetic daemon listening on http://127.0.0.1:${server.address().port}`)
)
