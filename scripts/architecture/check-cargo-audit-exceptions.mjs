#!/usr/bin/env node

// Keeps every entry of .cargo/audit.toml tied to the facts that justify it.
// An ignored advisory is only acceptable while the reason it was accepted still
// holds, so this fails when the dependency moves, when the vulnerable code path
// becomes reachable, or when an entry appears that has no justification here.
//
// The libcrux and quick-xml exceptions were removed once Engine v1.1.0-rc.21
// brought hpke-rs 0.7 and quick-xml 0.41: those advisories are fixed, not waived.

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = resolve(SCRIPT_DIR, '../..')

// RUSTSEC-2023-0071: the Marvin Attack timing side-channel in `rsa`. It arrives
// through jsonwebtoken and upstream still ships no fixed release, so the entry
// stays until one does. It is accepted only because the RSA code path is never
// entered: the JWT layer signs and verifies with HS256 (HMAC) alone.
const EXPECTED_ADVISORIES = ['RUSTSEC-2023-0071']

// The advisory applies to this crate; a version bump means the exception has to
// be re-reviewed against whatever upstream changed.
const REVIEWED_VERSIONS = new Map([['rsa', '0.9.10']])

// Files that decide which JWT algorithms are used. Any RSA-family algorithm here
// would make the vulnerable path reachable and void the exception.
const JWT_ALGORITHM_SOURCES = ['crates/uc-webserver/src/security/claims.rs']
const RSA_JWT_ALGORITHM_PATTERN = /Algorithm::(?:RS|PS)\d{3}/

const RUST_SOURCE_ROOTS = ['apps', 'crates', 'tools']
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

function read(relativePath) {
  return readFileSync(join(REPOSITORY_ROOT, relativePath), 'utf8')
}

function addProblem(problems, check, message) {
  problems.push(`${check}: ${message}`)
}

function checkVersions(metadata, problems) {
  for (const [name, expectedVersion] of REVIEWED_VERSIONS) {
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

// The exception rests on the RSA signing code never running, which holds as long
// as no workspace crate depends on `rsa` directly and no JWT call site selects an
// RSA algorithm.
function checkVulnerablePathStaysUnreachable(metadata, problems) {
  const workspaceMembers = new Set(metadata.workspace_members)
  for (const pkg of metadata.packages.filter(candidate => workspaceMembers.has(candidate.id))) {
    for (const dependency of pkg.dependencies) {
      if (REVIEWED_VERSIONS.has(dependency.name)) {
        addProblem(
          problems,
          'direct dependency',
          `${pkg.name} now directly depends on ${dependency.name}`
        )
      }
    }
  }

  for (const source of JWT_ALGORITHM_SOURCES) {
    if (RSA_JWT_ALGORITHM_PATTERN.test(read(source))) {
      addProblem(problems, 'affected API use', `${source} selects an RSA JWT algorithm`)
    }
  }

  const pendingDirectories = RUST_SOURCE_ROOTS.map(root => join(REPOSITORY_ROOT, root))
  while (pendingDirectories.length > 0) {
    const directory = pendingDirectories.pop()
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolutePath = join(directory, entry.name)
      if (entry.isDirectory()) {
        if (entry.name !== 'target') pendingDirectories.push(absolutePath)
        continue
      }
      if (!entry.isFile() || !entry.name.endsWith('.rs')) continue
      if (RSA_JWT_ALGORITHM_PATTERN.test(readFileSync(absolutePath, 'utf8'))) {
        addProblem(
          problems,
          'affected API use',
          `${relative(REPOSITORY_ROOT, absolutePath)} selects an RSA JWT algorithm`
        )
      }
    }
  }
}

// `rsa` must stay a transitive dependency of the compiled graph on every shipped
// target; if it ever became a root of its own the reachability argument changes.
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

  for (const [name, version] of REVIEWED_VERSIONS) {
    if (
      !new RegExp(`^${name} v${version.replaceAll('.', '\\.')}(?: \\(\\*\\))?$`, 'm').test(tree)
    ) {
      addProblem(
        problems,
        'production feature graph',
        `${name} v${version} is no longer on the reviewed path`
      )
    }
  }
}

function checkAuditConfiguration(problems) {
  const auditConfig = read('.cargo/audit.toml')
  const listed = new Set(
    [...auditConfig.matchAll(/^\s*"(RUSTSEC-\d{4}-\d{4})",?\s*$/gm)].map(match => match[1])
  )
  for (const advisory of EXPECTED_ADVISORIES) {
    if (!listed.has(advisory)) {
      addProblem(problems, 'audit configuration', `${advisory} is no longer explicitly tracked`)
    }
  }
  for (const advisory of listed) {
    if (!EXPECTED_ADVISORIES.includes(advisory)) {
      addProblem(
        problems,
        'audit configuration',
        `${advisory} is ignored without a justification in this guard`
      )
    }
  }
}

function main() {
  const problems = []
  const metadata = JSON.parse(run('cargo', ['metadata', '--format-version', '1', '--locked']))

  checkVersions(metadata, problems)
  checkVulnerablePathStaysUnreachable(metadata, problems)
  checkProductionFeatures(problems)
  checkAuditConfiguration(problems)

  if (problems.length > 0) {
    console.error('Cargo audit exception assumptions changed:')
    for (const problem of problems) console.error(`- ${problem}`)
    console.error('Re-review the exception, or drop it if the advisory now has a fix.')
    process.exitCode = 1
    return
  }

  console.log('Cargo audit exception assumptions remain valid.')
}

main()
