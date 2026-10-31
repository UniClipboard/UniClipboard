import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { readFile, mkdir, writeFile } from 'node:fs/promises'
import { inventory } from './assertion-source.mjs'

const sourceRoot = 'apps/gui-go/frontend/e2e'
const names = [
  'settings-sidebar',
  'settings-sidebar-motion',
  'appearance',
  'app-entry',
  'app-entry-kit',
  'history-row-hover',
  'typography',
  'mobile-connection',
  'theme-palette',
  'ui-kit',
  'visual-effects',
  'file-preview',
  'history-detail-regressions',
  'history-compact',
  'device-tasks',
  'settings-design',
  'settings-scroll-memory',
  'history-tag-menu',
  'macos-design',
  'startup-first-frame',
  'sidebar-animation',
]

const rows = []
for (const name of names) {
  const oldPath = `${sourceRoot}/${name}${name === 'history-detail-regressions' ? '' : '-browser'}.mjs`
  const newPath = `${sourceRoot}/tester-army/${name}.e2e.mjs`
  const original = inventory(
    execFileSync('git', ['show', `7ae0a059c9e2c3cf2dd753240da9ef65f4343d40:${oldPath}`], {
      encoding: 'utf8',
    }),
    oldPath
  )
  const replacement = inventory(await readFile(newPath, 'utf8'), newPath)
  assert.ok(
    replacement.assertions.length >= original.assertions.length,
    `${name}: lost assertion sites`
  )
  rows.push({
    name,
    oldPath,
    newPath,
    original,
    replacement,
    unchangedAssertions: original.assertions.filter(text =>
      replacement.assertions.some(next => next.replace(/\s+/g, '') === text.replace(/\s+/g, ''))
    ).length,
    limitation:
      'Static assertion-site inventory; runtime reports establish execution. Locator/API translations require source review.',
  })
}
await mkdir('target/tester-army-layout-report', { recursive: true })
await writeFile(
  'target/tester-army-layout-report/assertion-inventory.json',
  JSON.stringify(rows, null, 2) + '\n'
)
console.log(`Indexed ${rows.length} original/replacement pairs without dropping assertion sites`)
