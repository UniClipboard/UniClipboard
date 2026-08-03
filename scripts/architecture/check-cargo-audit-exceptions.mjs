#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = resolve(SCRIPT_DIR, '../..')

const EXPECTED_VERSIONS = new Map([
  ['hpke-rs', '0.6.1'],
  ['hpke-rs-libcrux', '0.6.1'],
  ['libcrux-aesgcm', '0.0.7'],
  ['libcrux-chacha20poly1305', '0.0.7'],
  ['libcrux-secrets', '0.0.5'],
  ['libcrux-sha3', '0.0.8'],
])

const EXPECTED_ADVISORIES = [
  'RUSTSEC-2026-0211',
  'RUSTSEC-2026-0209',
  'RUSTSEC-2026-0124',
  'RUSTSEC-2026-0212',
  'RUSTSEC-2026-0207',
  'RUSTSEC-2026-0208',
]

const INACTIVE_PRODUCTION_CRATES = ['libcrux-aesgcm', 'libcrux-chacha20poly1305']
const ACTIVE_REVIEWED_CRATES = ['libcrux-sha3 v0.0.8', 'libcrux-secrets v0.0.5']
const DIRECT_API_PATTERN = /\blibcrux_(?:aesgcm|chacha20poly1305|secrets|sha3)\b/
const RUST_SOURCE_ROOTS = ['apps', 'crates', 'src-tauri']
const PRODUCTION_TARGETS = [
  'x86_64-unknown-linux-gnu',
  'x86_64-pc-windows-msvc',
  'x86_64-apple-darwin',
  'wasm32-unknown-unknown',
]

function run(command, args) {
  return execFileSync(command, args, {
    cwd: REPOSITORY_ROOT,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
}

function cargoMetadata() {
  const output = run('cargo', ['metadata', '--format-version', '1', '--locked'])
  try {
    return JSON.parse(output)
  } catch (error) {
    throw new Error(`cargo metadata returned invalid JSON: ${String(error)}`, { cause: error })
  }
}

function addProblem(problems, check, message) {
  problems.push(`${check}: ${message}`)
}

function checkVersions(metadata, problems) {
  for (const [name, expectedVersion] of EXPECTED_VERSIONS) {
    const versions = [
      ...new Set(metadata.packages.filter(pkg => pkg.name === name).map(pkg => pkg.version)),
    ]
    if (versions.length !== 1 || versions[0] !== expectedVersion) {
      addProblem(
        problems,
        'dependency version',
        `${name} must remain at ${expectedVersion}; found ${versions.join(', ') || 'missing'}`
      )
    }
  }
}

function checkProductionFeatures(problems) {
  const tree = PRODUCTION_TARGETS.map(target =>
    run('cargo', [
      'tree',
      '--workspace',
      '--locked',
      '-e',
      'features,no-dev',
      '--prefix',
      'none',
      '--target',
      target,
    ])
  ).join('\n')

  for (const crate of INACTIVE_PRODUCTION_CRATES) {
    if (hasExactPackage(tree, crate)) {
      addProblem(problems, 'production feature graph', `${crate} became active`)
    }
  }
  for (const crate of ACTIVE_REVIEWED_CRATES) {
    const [name, version] = crate.split(' v')
    if (!hasExactPackage(tree, name, version)) {
      addProblem(problems, 'production feature graph', `${crate} is no longer on the reviewed path`)
    }
  }
}

function hasExactPackage(tree, name, version) {
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const escapedVersion = version?.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') ?? '\\S+'
  return new RegExp(`^${escapedName} v${escapedVersion}(?: \\(\\*\\))?$`, 'm').test(tree)
}

function checkDirectApiUse(metadata, problems) {
  const workspaceMembers = new Set(metadata.workspace_members)
  for (const pkg of metadata.packages.filter(candidate => workspaceMembers.has(candidate.id))) {
    for (const dependency of pkg.dependencies) {
      if (EXPECTED_VERSIONS.has(dependency.name) && dependency.name !== 'hpke-rs') {
        addProblem(
          problems,
          'direct dependency',
          `${pkg.name} now directly depends on ${dependency.name}`
        )
      }
    }
  }

  const pendingDirectories = RUST_SOURCE_ROOTS.map(root => join(REPOSITORY_ROOT, root))
  while (pendingDirectories.length > 0) {
    const directory = pendingDirectories.pop()
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolutePath = join(directory, entry.name)
      if (entry.isDirectory()) {
        pendingDirectories.push(absolutePath)
        continue
      }
      if (!entry.isFile() || !entry.name.endsWith('.rs')) continue

      const source = readFileSync(absolutePath, 'utf8')
      const relativePath = relative(REPOSITORY_ROOT, absolutePath)
      if (DIRECT_API_PATTERN.test(source)) {
        addProblem(problems, 'affected API use', `${relativePath} directly references libcrux APIs`)
      }
    }
  }
}

function checkAuditConfiguration(problems) {
  const auditConfig = readFileSync(join(REPOSITORY_ROOT, '.cargo/audit.toml'), 'utf8')
  for (const advisory of EXPECTED_ADVISORIES) {
    if (!auditConfig.includes(`"${advisory}"`)) {
      addProblem(problems, 'audit configuration', `${advisory} is no longer explicitly tracked`)
    }
  }
}

function main() {
  const problems = []
  const metadata = cargoMetadata()

  checkVersions(metadata, problems)
  checkProductionFeatures(problems)
  checkDirectApiUse(metadata, problems)
  checkAuditConfiguration(problems)

  if (problems.length > 0) {
    console.error('Cargo audit exception assumptions changed:')
    for (const problem of problems) console.error(`- ${problem}`)
    console.error('Review issues #1464 through #1469 and remove fixed audit exceptions.')
    process.exitCode = 1
    return
  }

  console.log('Cargo audit exception assumptions remain valid.')
}

main()
