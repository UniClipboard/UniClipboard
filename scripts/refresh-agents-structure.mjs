#!/usr/bin/env node
/**
 * Pre-commit hook script: refresh the ## STRUCTURE section in crates/AGENTS.md
 * when workspace crate membership changes.
 *
 * Triggered by lint-staged on root Cargo.toml changes.
 * Only rewrites the STRUCTURE block; other sections are untouched.
 */

import { readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs'
import { join, basename } from 'node:path'
import { execSync } from 'node:child_process'

const ROOT = new URL('..', import.meta.url).pathname.replace(/\/$/, '')
const AGENTS_PATH = join(ROOT, 'crates', 'AGENTS.md')
const WORKSPACE_TOML = join(ROOT, 'Cargo.toml')

// -- 1. Parse workspace members from Cargo.toml --

function parseWorkspaceMembers() {
  const content = readFileSync(WORKSPACE_TOML, 'utf8')
  const match = content.match(/\[workspace\]\s*\nmembers\s*=\s*\[([\s\S]*?)\]/)
  if (!match) {
    console.error('[refresh-agents-structure] Cannot parse workspace members')
    process.exit(0)
  }
  return match[1]
    .split('\n')
    .map(line => line.match(/"([^"]+)"/)?.[1])
    .filter(Boolean)
}

// -- 2. Extract one-line description from each crate's Cargo.toml or lib.rs --

const KNOWN_DESCRIPTIONS = {
  'uc-core': 'Domain models + Port traits only (no external deps)',
  'uc-application': 'Use cases / orchestrators (depends on uc-core ports only)',
  'uc-infra': 'Infra adapters: Diesel repos, iroh P2P, encryption, fs, timers',
  'uc-platform': 'OS adapters: clipboard, secure storage, autostart',
  'uc-observability': 'Dual-output tracing, profile filtering, Sentry/analytics scope',
  'uc-bootstrap':
    'Composition root -- the ONLY crate that may depend on core+app+infra+platform at once',
  'uc-app-paths': 'Lightweight directory-layout authority (data/cache/tmp)',
  'uc-webserver': "Daemon's 127.0.0.1 HTTP + WebSocket API (OpenAPI / ApiEnvelope)",
  'uc-daemon-contract': 'Transport DTOs/contracts shared by client + server',
  'uc-daemon-process': 'Thin process primitives: PID file, socket path, spawn, health-wait',
  'uc-daemon': 'GUI-agnostic daemon runtime; hosts the `uniclipd` binary',
  'uc-daemon-local': 'Local process coordination: auth token, socket discovery, health polling',
  'uc-daemon-client': 'Daemon HTTP + WS client (used by GUI + CLI)',
  'uc-desktop': 'Desktop host: runtime, daemon probe, background tasks (GUI-framework-agnostic)',
  'uc-tauri': 'Tauri adapter: commands (via tauri-specta), tray, quick panel, run loop',
  'uc-cli': '`uniclip` CLI (daemon client; heavy deps feature-gated)',
  'uc-cli-macros': 'Proc-macros for uc-cli (internal)',
  'p2p-bench': 'Throwaway perf-spike bins (not shipped; publish = false)',
  uniclipboard: 'Tauri desktop bin package (packaging shell; hands off to uc-tauri)',
}

function getDescription(cratePath) {
  const name = packageName(cratePath)
  if (KNOWN_DESCRIPTIONS[name]) return KNOWN_DESCRIPTIONS[name]

  // Fallback: try to read `description` from crate's Cargo.toml
  const tomlPath = join(ROOT, cratePath, 'Cargo.toml')
  if (existsSync(tomlPath)) {
    const toml = readFileSync(tomlPath, 'utf8')
    const desc = toml.match(/^description\s*=\s*"([^"]+)"/m)
    if (desc) return desc[1]
  }
  return '(no description)'
}

// -- 3. Group crates by architectural layer --

const LAYER_ORDER = [
  {
    comment: 'Hex core (ADR-005)',
    members: [
      'uc-core',
      'uc-application',
      'uc-infra',
      'uc-platform',
      'uc-observability',
      'uc-app-paths',
      'uc-bootstrap',
    ],
  },
  {
    comment: 'Daemon split (ADR-007/008)',
    members: [
      'uc-webserver',
      'uc-daemon-contract',
      'uc-daemon-process',
      'uc-daemon',
      'uc-daemon-local',
      'uc-daemon-client',
    ],
  },
  {
    comment: 'Shells / entrypoints',
    members: ['uc-desktop', 'uc-tauri', 'uc-cli', 'uc-cli-macros', 'p2p-bench'],
  },
]

// Resolve the cargo package name (dir basename != package name for apps/*).
function packageName(cratePath) {
  const tomlPath = join(ROOT, cratePath, 'Cargo.toml')
  if (existsSync(tomlPath)) {
    const m = readFileSync(tomlPath, 'utf8').match(/^name\s*=\s*"([^"]+)"/m)
    if (m) return m[1]
  }
  return basename(cratePath)
}

function categorizeMember(cratePath) {
  const name = packageName(cratePath)
  for (const layer of LAYER_ORDER) {
    if (layer.members.includes(name)) return layer
  }
  return null
}

// -- 4. Generate STRUCTURE block --

function generateStructure(members) {
  const apps = members.filter(m => m.startsWith('apps/'))
  const libs = members.filter(m => m.startsWith('crates/'))
  const tauri = members.filter(m => m === 'src-tauri' || m.startsWith('src-tauri/'))
  const crateCount = members.length

  const lines = [
    '```text',
    '.                        # repo root = cargo workspace',
    '|- apps/                 # Runnable binaries',
  ]
  for (const m of apps) {
    const dir = m.slice('apps/'.length)
    const padding = Math.max(1, 20 - dir.length)
    lines.push(`|  |- ${dir}/${' '.repeat(padding)}# ${getDescription(m)}`)
  }

  lines.push(`|- crates/               # Library crates (${libs.length})`)

  // Group library members by layer
  const layered = new Map()
  const uncategorized = []
  for (const m of libs) {
    const layer = categorizeMember(m)
    if (layer) {
      if (!layered.has(layer.comment)) layered.set(layer.comment, [])
      layered.get(layer.comment).push(m)
    } else {
      uncategorized.push(m)
    }
  }

  for (const layer of LAYER_ORDER) {
    const crates = layered.get(layer.comment)
    if (!crates || crates.length === 0) continue
    lines.push(`|  # -- ${layer.comment} --`)
    for (const cratePath of crates) {
      const name = basename(cratePath)
      const padding = Math.max(1, 17 - name.length)
      lines.push(`|  |- ${name}/${' '.repeat(padding)}# ${getDescription(cratePath)}`)
    }
  }
  if (uncategorized.length > 0) {
    lines.push('|  # -- Other --')
    for (const m of uncategorized) {
      const name = basename(m)
      const padding = Math.max(1, 17 - name.length)
      lines.push(`|  |- ${name}/${' '.repeat(padding)}# ${getDescription(m)}`)
    }
  }

  lines.push(
    '|- src-tauri/            # Desktop GUI app (Tauri packaging shell; dir name pinned by tauri-cli)'
  )
  for (const m of tauri) {
    if (m === 'src-tauri') {
      lines.push(
        '|  |- src/               # Thin bin: hands off to uc_tauri::run(generate_context!())'
      )
    } else {
      const name = basename(m)
      lines.push(`|  \`- crates/${name}/    # ${getDescription(m)}`)
    }
  }
  lines.push('`- crates/uc-infra/migrations/ # Active infra (diesel) migrations')
  lines.push('```')

  return lines.join('\n')
}

// -- 5. Replace section in AGENTS.md --

function replaceSection(content, newStructure) {
  const startMarker = '## STRUCTURE'
  const endMarker = /\n## [A-Z]/

  const startIdx = content.indexOf(startMarker)
  if (startIdx === -1) {
    console.error('[refresh-agents-structure] ## STRUCTURE section not found')
    process.exit(0)
  }

  const afterStart = content.slice(startIdx + startMarker.length)
  const endMatch = afterStart.match(endMarker)
  if (!endMatch) {
    console.error('[refresh-agents-structure] Cannot find next section after STRUCTURE')
    process.exit(0)
  }

  const endIdx = startIdx + startMarker.length + endMatch.index
  const before = content.slice(0, startIdx + startMarker.length)
  const after = content.slice(endIdx)

  return `${before}\n\n${newStructure}\n\n${after}`
}

// -- 6. Update the "Last refreshed" line --

function updateRefreshDate(content, crateCount) {
  const today = new Date().toISOString().slice(0, 10)
  const refreshLine = `**Last refreshed:** ${today} (auto; ${crateCount} workspace crates)`
  return content.replace(/\*\*Last refreshed:\*\*.*$/m, refreshLine)
}

// -- Main --

function main() {
  const members = parseWorkspaceMembers()
  const structure = generateStructure(members)
  let content = readFileSync(AGENTS_PATH, 'utf8')
  const newContent = updateRefreshDate(replaceSection(content, structure), members.length)

  if (newContent === content) {
    // No change needed
    process.exit(0)
  }

  writeFileSync(AGENTS_PATH, newContent)

  // Stage the updated file so it's included in the commit
  try {
    execSync('git add crates/AGENTS.md', { cwd: ROOT, stdio: 'pipe' })
  } catch {
    // If git add fails (e.g., not in a git context), just write the file
  }

  console.log(`[refresh-agents-structure] Updated STRUCTURE (${members.length} crates)`)
}

main()
