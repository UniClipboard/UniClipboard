#!/usr/bin/env node

// Every `@tauri-apps/*` module the shared frontend imports must be aliased in apps/gui-go/frontend/vite.config.ts to a host
// adapter, and each alias must point at an existing adapter file. A package that is imported but not aliased would
// be bundled from node_modules and call a Tauri runtime that does not exist; the failure would only show at runtime
// on the first call. Test files are excluded: they mock these modules.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const SOURCES = [join(ROOT, 'apps/gui-go/frontend/src')]
const VITE_CONFIG = join(ROOT, 'apps/gui-go/frontend/vite.config.ts')

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'dist') continue
    const path = join(dir, name)
    if (statSync(path).isDirectory()) walk(path, out)
    else if (/\.(ts|tsx)$/.test(path) && !/__tests__|\.test\./.test(path)) out.push(path)
  }
  return out
}

const config = readFileSync(VITE_CONFIG, 'utf8')
const aliases = new Map()
for (const match of config.matchAll(
  /find:\s*'(@tauri-apps\/[^']+)',\s*replacement:\s*host\('([^']+)'\)/g
)) {
  aliases.set(match[1], match[2])
}

const imported = new Map() // package id -> first file
for (const dir of SOURCES) {
  for (const file of walk(dir)) {
    const source = readFileSync(file, 'utf8')
    for (const match of source.matchAll(/(?:from|import\()\s*['"](@tauri-apps\/[^'"]+)['"]/g)) {
      if (!imported.has(match[1])) imported.set(match[1], relative(ROOT, file))
    }
  }
}

const problems = []
for (const [id, file] of imported) {
  if (!aliases.has(id))
    problems.push(`${file}: imports ${id}, which apps/gui-go/frontend/vite.config.ts does not alias`)
}
for (const [id, adapter] of aliases) {
  if (!existsSync(join(ROOT, 'apps/gui-go/frontend/src/host', `${adapter}.ts`))) {
    problems.push(`${id} is aliased to host/${adapter}, which does not exist`)
  }
}

if (problems.length > 0) {
  console.error(`@tauri-apps alias check failed:\n${problems.map(p => `  - ${p}`).join('\n')}`)
  process.exit(1)
}
console.log(`@tauri-apps aliases ok (${imported.size} imported, ${aliases.size} aliased)`)
