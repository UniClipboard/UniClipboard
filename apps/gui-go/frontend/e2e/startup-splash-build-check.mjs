// Nonvisual build E2E. Failure modes: missing HTML anchors must fail; indentation must not
// silently retain the legacy root; macOS/Windows/Linux select their skin; no-platform retains legacy.
import assert from 'node:assert/strict'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { build, loadConfigFromFile } from 'vite'

const output = path.resolve(process.argv[2])
await mkdir(output, { recursive: true })
process.chdir(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'))
const cases = [
  ['macos', 'missing-root', html => html.replace('id="root"', 'id="missing-root"'), 'root'],
  [
    'macos',
    'missing-copy',
    html => html.replace('id="uc-splash-copy"', 'id="missing-copy"'),
    'copy',
  ],
  ['macos', 'missing-style', html => html.replace(/<style>[\s\S]*?<\/style>/, ''), 'style'],
  [
    'macos',
    'reformatted',
    html => html.replace('    <script type="application/json"', '<script type="application/json"'),
  ],
  ['windows', 'normal', html => html],
  ['linux', 'normal', html => html],
  ['none', 'normal', html => html],
]
const results = []
try {
  for (const [platform, scenario, mutate, missing] of cases) {
    if (platform === 'none') delete process.env.UC_UI_PLATFORM
    else process.env.UC_UI_PLATFORM = platform
    const loaded = await loadConfigFromFile({ command: 'build', mode: 'production' })
    let html
    let failure
    try {
      const bundle = await build({
        ...loaded.config,
        configFile: false,
        logLevel: 'silent',
        plugins: [
          {
            name: 'startup-html-fixture',
            transformIndexHtml: {
              order: 'pre',
              handler: (html, context) =>
                context.filename.endsWith('/index.html') ? mutate(html) : html,
            },
          },
          ...loaded.config.plugins,
        ],
        build: { ...loaded.config.build, write: false },
      })
      html = bundle.output.find(item => item.fileName === 'index.html').source
    } catch (error) {
      failure = error.message
    }
    results.push({
      platform,
      scenario,
      failure: failure ?? null,
      skin: html?.match(/data-ui-skin="([^"]+)"/)?.[1] ?? null,
    })
    if (missing)
      assert.match(failure ?? '', new RegExp(`uc-startup-splash: missing ${missing} HTML anchor`))
    else {
      assert.equal(failure, undefined)
      if (platform === 'none') assert.ok(!html.includes('data-ui-skin='))
      else assert.ok(html.includes(`data-ui-skin="${platform}"`))
    }
  }
  console.log('Startup splash build checks passed (no browser or App launched).')
} finally {
  await writeFile(path.join(output, 'results.json'), JSON.stringify(results, null, 2))
}
