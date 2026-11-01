import { execFileSync } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { inventory } from './assertion-source.mjs'

const base = '7ae0a059c9e2c3cf2dd753240da9ef65f4343d40'
const root = 'apps/gui-go/frontend/e2e'
const names = [
  'history-kit',
  'devices-kit',
  'history-search',
  'tag-manager-layout',
  'quick-panel-unlock',
  'sidebar-toggle-app',
]
const rows = []
for (const name of names) {
  const originalPath = `${root}/${name}-browser.mjs`
  const original = inventory(
    execFileSync('git', ['show', `${base}:${originalPath}`], { encoding: 'utf8' }),
    originalPath
  )
  const owners = [`${root}/tester-army/${name}.e2e.mjs`]
  if (['history-kit', 'devices-kit'].includes(name))
    owners.push(originalPath, `${root}/browser-daemon-session.mjs`)
  if (name === 'devices-kit')
    owners.push(`${root}/devices-kit-lifecycle.mjs`, `${root}/devices-kit-passphrase-lifecycle.mjs`)
  const sources = {}
  for (const file of owners) sources[file] = inventory(await readFile(file, 'utf8'), file)
  rows.push({
    name,
    base,
    originalPath,
    original,
    sources,
    assertionOwnership: ['history-kit', 'devices-kit'].includes(name)
      ? 'shared original scenario owner; legacy CLI retains native/bootstrap ownership'
      : 'replacement candidate; original path retained until passing',
    limitation:
      'Inventory records complete source and assertions. Driver and lifecycle translations require semantic review; counts alone do not establish equivalence or execution.',
  })
}
await mkdir('target/tester-army-backend-report', { recursive: true })
await writeFile(
  'target/tester-army-backend-report/assertion-inventory.json',
  JSON.stringify(rows, null, 2) + '\n'
)
console.log(`Recorded ${rows.length} original backend suites and their assertion owners`)
