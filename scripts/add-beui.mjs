#!/usr/bin/env node
// Pull beUI components (https://beui.dev) into src/.
//
// beUI is a copy-paste component registry, not a standard shadcn registry, so
// the shadcn CLI cannot add its items. This script fetches a component's JSON
// from the registry, writes each file under src/ (beUI paths are src-relative
// and its `@/...` imports already match our `@ -> src` alias), and rewrites its
// motion imports for this project: `motion/react` -> `framer-motion` (the package
// we ship), and the full `motion` component -> the lightweight `m` component
// (required because every React root here runs under <LazyMotion strict>).
//
// Usage:
//   node scripts/add-beui.mjs <slug> [<slug>...] [--no-install]
//   bun run add:beui <slug> [<slug>...]
//
// Browse available slugs at https://beui.dev/r
//
// Safety: existing files are never overwritten (skipped). This protects
// src/lib/utils.ts — beUI ships its own cn() as lib/utils.ts, but ours also
// holds project helpers like formatPeerIdForDisplay. Shared beUI utils
// (lib/ease.ts, ...) are written once by the first component and skipped after.

import { execFileSync } from 'node:child_process'
import { mkdir, writeFile, readFile, access } from 'node:fs/promises'
import { dirname, resolve, join, sep } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(ROOT, 'src')
const REGISTRY = 'https://beui.dev/r'

// beUI lists `motion` as a dependency, but we rewrite its imports to
// framer-motion, so the motion package is never actually needed.
const IGNORED_DEPS = new Set(['motion'])

const args = process.argv.slice(2)
const noInstall = args.includes('--no-install')
const slugs = args.filter(a => !a.startsWith('-'))

if (slugs.length === 0) {
  console.error('Usage: node scripts/add-beui.mjs <slug> [<slug>...] [--no-install]')
  console.error('Browse slugs at https://beui.dev/r')
  process.exit(1)
}

let pkg
try {
  pkg = JSON.parse(await readFile(join(ROOT, 'package.json'), 'utf8'))
} catch (error) {
  console.error(
    `Failed to read package.json: ${error instanceof Error ? error.message : String(error)}`
  )
  process.exit(1)
}

const owned = new Set([
  ...Object.keys(pkg.dependencies ?? {}),
  ...Object.keys(pkg.devDependencies ?? {}),
])

const exists = p =>
  access(p).then(
    () => true,
    () => false
  )

const rewriteMotion = content => {
  // 1. Module path: beUI imports from "motion/react"; this project ships framer-motion.
  let out = content
    .replace(/(["'])motion\/react\1/g, '$1framer-motion$1')
    .replace(/(from\s+["'])motion(["'])/g, '$1framer-motion$2')

  // 2. Every React root in this project renders under <LazyMotion strict>, which
  //    forbids the full `motion` component (it throws at runtime) — the
  //    lightweight `m` component must be used instead. Rewrite the `motion` named
  //    import from framer-motion and every `motion.<tag>` usage to `m`. This
  //    leaves useReducedMotion / MotionConfig / HTMLMotionProps / AnimatePresence
  //    and the "framer-motion" string untouched. (This step is only valid because
  //    this project guarantees a LazyMotion ancestor; it is not generally safe.)
  out = out.replace(
    /import\s*\{([\s\S]*?)\}\s*from\s*(["'])framer-motion\2/g,
    (_full, names, q) =>
      `import {${names.replace(/(^|[,\s])motion(\s*(?:,|$))/g, '$1m$2')}} from ${q}framer-motion${q}`
  )
  out = out.replace(/\bmotion\.(?=[a-zA-Z])/g, 'm.')

  return out
}

const missingDeps = new Set()
const written = []
const skipped = []

for (const slug of slugs) {
  const url = `${REGISTRY}/${slug}`
  const res = await fetch(url)
  if (!res.ok) {
    console.error(`✗ ${slug}: ${res.status} ${res.statusText} (${url})`)
    process.exitCode = 1
    continue
  }
  const item = await res.json()
  console.log(`\n▸ ${item.name} — ${item.description ?? ''}`)

  for (const file of item.files ?? []) {
    if (file.type === 'preview') {
      skipped.push(`${file.path} (preview/example)`)
      continue
    }
    const target = join(SRC, file.path)
    // Sandbox: registry data is untrusted; reject paths that escape src/.
    if (!resolve(target).startsWith(SRC + sep)) {
      console.error(`✗ ${file.path}: resolves outside src/, skipping`)
      process.exitCode = 1
      continue
    }
    if (await exists(target)) {
      skipped.push(`${file.path} (already exists)`)
      continue
    }
    await mkdir(dirname(target), { recursive: true })
    await writeFile(target, rewriteMotion(file.content))
    written.push(`src/${file.path}`)
  }

  for (const dep of item.dependencies ?? []) {
    if (!owned.has(dep) && !IGNORED_DEPS.has(dep)) missingDeps.add(dep)
  }
}

console.log('\n── summary ──')
if (written.length === 0) console.log('  (no new files written)')
written.forEach(p => console.log(`  + ${p}`))
skipped.forEach(p => console.log(`  · skipped ${p}`))

if (missingDeps.size === 0) {
  console.log('\nNo new dependencies needed.')
} else if (noInstall) {
  console.log(`\nMissing deps — install manually:\n  bun add ${[...missingDeps].join(' ')}`)
} else {
  const list = [...missingDeps]
  console.log(`\nInstalling missing deps: ${list.join(' ')}`)
  execFileSync('bun', ['add', ...list], { cwd: ROOT, stdio: 'inherit' })
}

console.log('\nDone. Import from "@/components/motion/...".')
