#!/usr/bin/env node

// Cross-checks the host event contract from its real sources, so no list is kept by hand:
//   - Go side: apps/gui-go/host_events.go declares each event as a constant (name, direction comment) and registers
//     it with application.RegisterEvent; apps/gui-go/*.go emit or listen through those constants.
//   - Page side: apps/gui-go/frontend/src calls listen(...) / Events.On(...).
// Failures: a page listener for an event the host never declares; a declared event nobody emits (unless it is in
// UNEMITTED with the reason); a declared host->page event no page listens to; a declared event that is not
// registered with RegisterEvent (so Wails would not type it); an emit of a literal name instead of the constant.

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const GO_DIR = join(ROOT, 'apps/gui-go')
const PAGE_DIRS = [join(ROOT, 'apps/gui-go/frontend/src')]

// Declared events that have a page listener but no host emitter, with the reason. Adding to this list is a product
// decision, not a way to silence the check.
const UNEMITTED = new Map([
  [
    'desktop-theme://changed',
    'Omarchy theming is not implemented in the Go host (GetDesktopTheme always reports it unavailable), so nothing can change the desktop theme at runtime. The page listener is gated on availability.',
  ],
])

// Names Wails or the window system emit by themselves; listening to them needs no host declaration.
const FRAMEWORK_EVENT = /^(common:|windows:|linux:|mac:|tauri:\/\/)/

function walk(dir, accept, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'dist' || name === 'bindings' || name === 'generated')
      continue
    const path = join(dir, name)
    if (statSync(path).isDirectory()) walk(path, accept, out)
    else if (accept(path)) out.push(path)
  }
  return out
}

const problems = []
const rel = path => relative(ROOT, path)

// ---- Go declarations
const eventsSource = readFileSync(join(GO_DIR, 'host_events.go'), 'utf8')
const declared = new Map() // const name -> { name, direction }
for (const match of eventsSource.matchAll(/^\s*(\w+)\s*=\s*"([^"]+)"\s*\/\/\s*(.*)$/gm)) {
  const [, constant, name, comment] = match
  const direction = /page -> host/.test(comment)
    ? 'page->host'
    : /both ways/.test(comment)
      ? 'both'
      : 'host->page'
  declared.set(constant, { name, direction })
}
const registered = new Set(
  [...eventsSource.matchAll(/RegisterEvent\[[^\]]*\]\((\w+)\)/g)].map(m => m[1])
)
for (const constant of declared.keys()) {
  if (!registered.has(constant))
    problems.push(`${constant} is declared but not registered with RegisterEvent`)
}

// ---- Go emitters and listeners
const goFiles = readdirSync(GO_DIR)
  .filter(name => name.endsWith('.go') && !name.endsWith('_test.go') && name !== 'host_events.go')
  .map(name => join(GO_DIR, name))
const emitted = new Set()
const goListened = new Set()
for (const file of goFiles) {
  const source = readFileSync(file, 'utf8')
  for (const match of source.matchAll(/\b(?:emit|Emit)\(\s*("?)([\w:/.\-]+)\1/g)) {
    if (match[1] === '"')
      problems.push(`${rel(file)}: emits the literal event name "${match[2]}"; use its constant`)
    else emitted.add(match[2])
  }
  for (const match of source.matchAll(/Event\.On\(\s*("?)([\w:/.\-]+)\1/g)) {
    if (match[1] === '"')
      problems.push(
        `${rel(file)}: listens to the literal event name "${match[2]}"; use its constant`
      )
    else goListened.add(match[2])
  }
}

// ---- page listeners
const pageFiles = PAGE_DIRS.flatMap(dir =>
  walk(dir, p => /\.(ts|tsx)$/.test(p) && !/__tests__|\.test\./.test(p))
)
const constants = new Map()
for (const file of pageFiles) {
  for (const match of readFileSync(file, 'utf8').matchAll(/\bconst\s+(\w+)\s*=\s*'([^']+)'/g)) {
    constants.set(match[1], match[2])
  }
}
const pageListened = new Map() // event name -> first file
for (const file of pageFiles) {
  const source = readFileSync(file, 'utf8')
  for (const match of source.matchAll(
    /\b(?:listen|Events\.On|Events\.Once)(?:<[^()]*>)?\(\s*(?:'([^']+)'|(\w+))/g
  )) {
    const name = match[1] ?? constants.get(match[2])
    if (name === undefined) continue // an identifier that is not a string constant (a parameter)
    if (!pageListened.has(name)) pageListened.set(name, rel(file))
  }
}

// ---- rules
const byName = new Map([...declared.values()].map(event => [event.name, event]))
for (const [name, file] of pageListened) {
  if (FRAMEWORK_EVENT.test(name)) continue
  if (!byName.has(name))
    problems.push(`${file}: listens to "${name}", which no host event declares`)
}
for (const [constant, event] of declared) {
  const emits = emitted.has(constant)
  const pageSide = pageListened.has(event.name)
  const hostSide = goListened.has(constant)
  if (event.direction !== 'page->host' && !emits && !UNEMITTED.has(event.name)) {
    problems.push(`${event.name} (${constant}) is never emitted by the host`)
  }
  if (event.direction !== 'page->host' && emits && !pageSide) {
    problems.push(`${event.name} (${constant}) is emitted but no page listens to it`)
  }
  if (event.direction === 'page->host' && !hostSide) {
    problems.push(
      `${event.name} (${constant}) is sent by the page but the host never listens to it`
    )
  }
  if (UNEMITTED.has(event.name) && emits) {
    problems.push(`${event.name} is emitted now: remove it from UNEMITTED`)
  }
}

if (problems.length > 0) {
  console.error(`host event contract check failed:\n${problems.map(p => `  - ${p}`).join('\n')}`)
  process.exit(1)
}
console.log(
  `host event contract ok (${declared.size} declared, ${emitted.size} emitted, ${pageListened.size} page listeners)`
)
