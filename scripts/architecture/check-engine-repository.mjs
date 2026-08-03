#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = resolve(SCRIPT_DIR, '../..')
const ENGINE_REPOSITORY = 'https://github.com/UniClipboard/Engine.git'
const ENGINE_REVISION = 'e5101db6f470e6b2e539d314d1e3581538dcec62'
const DECLARED_ENGINE_SOURCE = `git+${ENGINE_REPOSITORY}?rev=${ENGINE_REVISION}`
const RESOLVED_ENGINE_SOURCE = `${DECLARED_ENGINE_SOURCE}#${ENGINE_REVISION}`

const MIGRATED_PACKAGES = new Set([
  'uc-engine',
  'uc-core',
  'uc-application',
  'uc-infra',
  'uc-content-hash',
  'uc-observability-contract',
  'uc-engine-uniffi',
  'uc-ohos-napi',
  'uc-mobile-proto',
  'uc-mobile',
  'uc-mobile-probe-core',
])

const FORBIDDEN_RUNTIME_PACKAGES = new Set([
  'uc-core',
  'uc-application',
  'uc-infra',
  'uc-content-hash',
  'uc-mobile-proto',
  'uc-mobile',
])

const MIGRATED_PATHS = [
  'crates/uc-engine',
  'crates/uc-core',
  'crates/uc-application',
  'crates/uc-infra',
  'crates/uc-content-hash',
  'crates/uc-observability-contract',
  'crates/uc-engine-uniffi',
  'crates/uc-ohos-napi',
  'crates/uc-mobile-proto',
  'crates/uc-mobile',
  'apps/mobile-probe-core',
]

function read(relativePath) {
  return readFileSync(join(REPOSITORY_ROOT, relativePath), 'utf8')
}

function cargoMetadata() {
  const output = execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
    cwd: REPOSITORY_ROOT,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
  try {
    return JSON.parse(output)
  } catch (error) {
    throw new Error(`cargo metadata returned invalid JSON: ${String(error)}`, { cause: error })
  }
}

function workspacePackages(metadata) {
  const members = new Set(metadata.workspace_members)
  return metadata.packages.filter(candidate => members.has(candidate.id))
}

function workspacePackageByName(metadata, name) {
  const found = workspacePackages(metadata).find(candidate => candidate.name === name)
  if (!found) throw new Error(`workspace package is missing: ${name}`)
  return found
}

function dependency(packageMetadata, dependencyName, kind = null) {
  return packageMetadata.dependencies.find(
    candidate => candidate.name === dependencyName && candidate.kind === kind
  )
}

function addProblem(problems, check, message) {
  problems.push(`${check}: ${message}`)
}

function checkRepositoryBoundary(metadata, { checkPaths = true } = {}) {
  const problems = []

  for (const packageMetadata of workspacePackages(metadata)) {
    if (MIGRATED_PACKAGES.has(packageMetadata.name)) {
      addProblem(
        problems,
        'repository boundary',
        `${packageMetadata.name} remains a desktop workspace package`
      )
    }

    for (const item of packageMetadata.dependencies) {
      if (!MIGRATED_PACKAGES.has(item.name)) continue
      if (item.path) {
        addProblem(
          problems,
          'repository boundary',
          `${packageMetadata.name} uses a local path for ${item.name}`
        )
      }
      if (item.source !== DECLARED_ENGINE_SOURCE) {
        addProblem(
          problems,
          'engine provenance',
          `${packageMetadata.name} does not pin ${item.name} to ${ENGINE_REVISION}`
        )
      }
    }
  }

  if (checkPaths) {
    for (const migratedPath of MIGRATED_PATHS) {
      if (existsSync(join(REPOSITORY_ROOT, migratedPath))) {
        addProblem(problems, 'repository boundary', `${migratedPath} still exists in desktop`)
      }
    }
  }

  const engineSources = new Set(
    metadata.packages
      .map(candidate => candidate.source)
      .filter(source => source?.startsWith(`git+${ENGINE_REPOSITORY}`))
  )
  if (engineSources.size !== 1 || !engineSources.has(RESOLVED_ENGINE_SOURCE)) {
    addProblem(
      problems,
      'engine provenance',
      `expected one resolved Engine source at revision ${ENGINE_REVISION}; found ${[...engineSources].join(', ')}`
    )
  }

  for (const packageMetadata of metadata.packages) {
    if (!MIGRATED_PACKAGES.has(packageMetadata.name)) continue
    if (packageMetadata.source !== RESOLVED_ENGINE_SOURCE) {
      addProblem(
        problems,
        'engine provenance',
        `${packageMetadata.name} resolved outside the pinned Engine revision`
      )
    }
  }

  return problems
}

function checkPublicSurface(metadata) {
  const problems = []

  for (const packageMetadata of workspacePackages(metadata)) {
    const forbidden = packageMetadata.dependencies
      .filter(item => item.kind === null && FORBIDDEN_RUNTIME_PACKAGES.has(item.name))
      .map(item => item.name)
    if (forbidden.length > 0) {
      addProblem(
        problems,
        'consumer firewall',
        `${packageMetadata.name} directly depends on ${forbidden.join(', ')}`
      )
    }
  }

  const requiredDependencies = [
    ['uc-daemon', 'uc-engine'],
    ['uc-bootstrap', 'uc-engine'],
    ['uc-webserver', 'uc-engine'],
    ['uc-cli', 'uc-engine'],
    ['uc-observability', 'uc-observability-contract'],
  ]
  for (const [packageName, dependencyName] of requiredDependencies) {
    if (!dependency(workspacePackageByName(metadata, packageName), dependencyName)) {
      addProblem(
        problems,
        'consumer firewall',
        `${packageName} must consume ${dependencyName} from the pinned Engine revision`
      )
    }
  }

  return problems
}

function checkLanIsolation(metadata, sources) {
  const problems = []
  const webserverEngine = dependency(workspacePackageByName(metadata, 'uc-webserver'), 'uc-engine')
  if (!webserverEngine?.features.includes('lan-compat')) {
    addProblem(problems, 'compatibility gate', 'uc-webserver must enable lan-compat explicitly')
  }

  for (const packageName of ['uc-daemon', 'uc-bootstrap']) {
    const engine = dependency(workspacePackageByName(metadata, packageName), 'uc-engine')
    if (engine?.features.includes('lan-compat')) {
      addProblem(
        problems,
        'compatibility gate',
        `${packageName} must not enable lan-compat directly`
      )
    }
  }

  const cli = workspacePackageByName(metadata, 'uc-cli')
  const cliEngine = dependency(cli, 'uc-engine')
  if (!cliEngine?.optional || !cliEngine.features.includes('dev-tools')) {
    addProblem(
      problems,
      'compatibility gate',
      'uc-cli must keep uc-engine optional and restricted to dev-tools'
    )
  }
  if (!(cli.features['dev-tools'] ?? []).includes('uc-engine/lan-compat')) {
    addProblem(
      problems,
      'compatibility gate',
      'uc-cli dev-tools must enable LAN compatibility explicitly'
    )
  }

  const targetSignature =
    'pub(crate) fn initial_lan_target(view: Option<&MobileSyncSettingsSummary>)'
  if (!sources.mobileLanLifecycle.includes(targetSignature)) {
    addProblem(
      problems,
      'compatibility gate',
      'initial LAN state must come only from explicit mobile-sync settings'
    )
  }
  if (!sources.engineEvents.includes('EngineEvent::MobileLanSettingsChanged(settings)')) {
    addProblem(
      problems,
      'compatibility gate',
      'runtime LAN changes must be driven by the explicit settings event'
    )
  }
  if (sources.engineEvents.includes('fallback_to_lan')) {
    addProblem(problems, 'compatibility gate', 'engine events contain an automatic LAN fallback')
  }

  return problems
}

function repositorySources() {
  return {
    mobileLanLifecycle: read('apps/daemon/src/daemon/mobile_lan_lifecycle.rs'),
    engineEvents: read('apps/daemon/src/daemon/engine_events.rs'),
  }
}

function collectProblems(metadata, sources, options) {
  return [
    ...checkRepositoryBoundary(metadata, options),
    ...checkPublicSurface(metadata),
    ...checkLanIsolation(metadata, sources),
  ]
}

function clone(value) {
  return structuredClone(value)
}

function expectRejected(name, mutate, metadata, sources) {
  const changedMetadata = clone(metadata)
  const changedSources = { ...sources }
  mutate(changedMetadata, changedSources)
  const problems = collectProblems(changedMetadata, changedSources, { checkPaths: false })
  if (problems.length === 0) throw new Error(`negative fixture was not rejected: ${name}`)
  process.stdout.write(`OK negative fixture rejected: ${name}\n`)
}

function runNegativeFixtures(metadata, sources) {
  expectRejected(
    'local core path dependency',
    changed => {
      const engine = dependency(workspacePackageByName(changed, 'uc-daemon'), 'uc-engine')
      engine.source = null
      engine.path = join(REPOSITORY_ROOT, 'crates/uc-engine')
    },
    metadata,
    sources
  )
  expectRejected(
    'tagged Engine dependency',
    changed => {
      const contract = dependency(
        workspacePackageByName(changed, 'uc-observability'),
        'uc-observability-contract'
      )
      contract.source = `git+${ENGINE_REPOSITORY}?tag=v0.20.0-rc.5`
    },
    metadata,
    sources
  )
  expectRejected(
    'automatic LAN fallback',
    (_changed, changedSources) => {
      changedSources.mobileLanLifecycle = changedSources.mobileLanLifecycle.replace(
        'pub(crate) fn initial_lan_target(view: Option<&MobileSyncSettingsSummary>)',
        'pub(crate) fn initial_lan_target(p2p_failed: bool, view: Option<&MobileSyncSettingsSummary>)'
      )
      changedSources.engineEvents += '\nfn fallback_to_lan() {}\n'
    },
    metadata,
    sources
  )
}

function main() {
  if (process.cwd() !== REPOSITORY_ROOT) {
    throw new Error(
      `run from repository root: ${relative(process.cwd(), REPOSITORY_ROOT) || REPOSITORY_ROOT}`
    )
  }

  const metadata = cargoMetadata()
  const sources = repositorySources()
  const problems = collectProblems(metadata, sources)
  if (problems.length > 0) {
    for (const problem of problems) process.stderr.write(`ERROR ${problem}\n`)
    process.exitCode = 1
    return
  }

  runNegativeFixtures(metadata, sources)
  process.stdout.write('Desktop Engine consumer preflight passed\n')
}

main()
