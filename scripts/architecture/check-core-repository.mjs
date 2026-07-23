#!/usr/bin/env node

import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const REPOSITORY_ROOT = resolve(SCRIPT_DIR, '../..')

const CORE_REPOSITORY_PACKAGES = new Set([
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
])

const INTERNAL_PACKAGES = new Set([
  'uc-core',
  'uc-application',
  'uc-infra',
  'uc-content-hash',
  'uc-observability-contract',
  'uc-mobile-proto',
  'uc-mobile',
])

const BINDING_PACKAGES = ['uc-engine-uniffi', 'uc-ohos-napi']
const P2P_CONSUMERS = ['uc-engine-uniffi', 'uc-ohos-napi', 'uc-mobile-probe-core']
const DESKTOP_CONSUMERS = [
  'uc-bootstrap',
  'uc-cli',
  'uc-daemon',
  'uc-daemon-client',
  'uc-daemon-contract',
  'uc-desktop',
  'uc-platform',
  'uc-tauri',
  'uc-webserver',
]

const DESKTOP_ONLY_PACKAGES = new Set([
  'uc-app-paths',
  'uc-bootstrap',
  'uc-daemon-client',
  'uc-daemon-contract',
  'uc-daemon-local',
  'uc-daemon-process',
  'uc-desktop',
  'uc-observability',
  'uc-platform',
  'uc-tauri',
  'uc-webserver',
])

function read(relativePath) {
  return readFileSync(join(REPOSITORY_ROOT, relativePath), 'utf8')
}

function cargoMetadata() {
  const output = execFileSync(
    'cargo',
    ['metadata', '--no-deps', '--format-version', '1', '--locked', '--offline'],
    { cwd: REPOSITORY_ROOT, encoding: 'utf8' }
  )
  return JSON.parse(output)
}

function packageByName(metadata, name) {
  const found = metadata.packages.find(candidate => candidate.name === name)
  if (!found) throw new Error(`workspace package is missing: ${name}`)
  return found
}

function normalDependencies(packageMetadata) {
  return packageMetadata.dependencies.filter(dependency => dependency.kind === null)
}

function normalDependency(packageMetadata, dependencyName) {
  return normalDependencies(packageMetadata).find(dependency => dependency.name === dependencyName)
}

function featureItems(packageMetadata, featureName) {
  return packageMetadata.features[featureName] ?? []
}

function addProblem(problems, check, message) {
  problems.push(`${check}: ${message}`)
}

function checkCoreDependencies(metadata) {
  const problems = []

  for (const name of CORE_REPOSITORY_PACKAGES) {
    const packageMetadata = packageByName(metadata, name)
    for (const dependency of normalDependencies(packageMetadata)) {
      if (dependency.path && !CORE_REPOSITORY_PACKAGES.has(dependency.name)) {
        addProblem(
          problems,
          'core dependency firewall',
          `${name} has repository-local dependency ${dependency.name}`
        )
      }
      if (DESKTOP_ONLY_PACKAGES.has(dependency.name)) {
        addProblem(
          problems,
          'core dependency firewall',
          `${name} depends on desktop-owned package ${dependency.name}`
        )
      }
    }
  }

  const packages = new Map(metadata.packages.map(item => [item.name, item]))
  const pending = ['uc-engine']
  const visited = new Set()
  while (pending.length > 0) {
    const name = pending.pop()
    if (visited.has(name)) continue
    visited.add(name)
    const packageMetadata = packages.get(name)
    if (!packageMetadata) continue
    for (const dependency of normalDependencies(packageMetadata)) {
      if (packages.has(dependency.name)) pending.push(dependency.name)
    }
  }
  for (const name of visited) {
    if (DESKTOP_ONLY_PACKAGES.has(name)) {
      addProblem(
        problems,
        'core dependency firewall',
        `uc-engine normal dependency closure contains ${name}`
      )
    }
  }

  return problems
}

function checkPublicSurface(metadata) {
  const problems = []

  for (const name of INTERNAL_PACKAGES) {
    const packageMetadata = packageByName(metadata, name)
    if (!Array.isArray(packageMetadata.publish) || packageMetadata.publish.length !== 0) {
      addProblem(problems, 'public surface', `${name} must set publish = false`)
    }
  }

  for (const bindingName of BINDING_PACKAGES) {
    const binding = packageByName(metadata, bindingName)
    const localDependencies = normalDependencies(binding)
      .filter(dependency => dependency.path)
      .map(dependency => dependency.name)
      .sort()
    if (JSON.stringify(localDependencies) !== JSON.stringify(['uc-engine'])) {
      addProblem(
        problems,
        'public surface',
        `${bindingName} local dependencies must be exactly uc-engine; found ${localDependencies.join(', ')}`
      )
    }
  }

  for (const consumerName of DESKTOP_CONSUMERS) {
    const consumer = packageByName(metadata, consumerName)
    const forbidden = normalDependencies(consumer)
      .map(dependency => dependency.name)
      .filter(name => INTERNAL_PACKAGES.has(name))
    if (forbidden.length > 0) {
      addProblem(
        problems,
        'consumer firewall',
        `${consumerName} directly depends on ${forbidden.join(', ')}`
      )
    }
  }

  return problems
}

function checkBindingProvenance(metadata, sources) {
  const problems = []
  const engineVersion = packageByName(metadata, 'uc-engine').version

  for (const bindingName of BINDING_PACKAGES) {
    const bindingVersion = packageByName(metadata, bindingName).version
    if (bindingVersion !== engineVersion) {
      addProblem(
        problems,
        'binding provenance',
        `${bindingName} version ${bindingVersion} differs from uc-engine ${engineVersion}`
      )
    }
  }

  for (const [name, source] of [
    ['UniFFI', sources.uniffi],
    ['HarmonyOS', sources.ohos],
  ]) {
    if (!source.includes('format!("core-v{}", env!("CARGO_PKG_VERSION"))')) {
      addProblem(
        problems,
        'binding provenance',
        `${name} binding version is not derived from its Cargo package version`
      )
    }
  }

  const requiredPackagingTokens = [
    'core-version.txt',
    'source-commit.txt',
    'rev-parse HEAD',
    'checksum',
  ]
  for (const [name, script] of [
    ['iOS', sources.iosPackaging],
    ['Android', sources.androidPackaging],
    ['HarmonyOS', sources.ohosPackaging],
  ]) {
    for (const token of requiredPackagingTokens) {
      if (!script.includes(token)) {
        addProblem(problems, 'binding provenance', `${name} packaging is missing ${token}`)
      }
    }
  }
  for (const token of ['assembleHar', 'UniClipboardEngine.har']) {
    if (!sources.ohosPackaging.includes(token)) {
      addProblem(problems, 'binding provenance', `HarmonyOS packaging is missing ${token}`)
    }
  }

  return problems
}

function checkLanIsolation(metadata, sources) {
  const problems = []
  const engine = packageByName(metadata, 'uc-engine')
  const application = packageByName(metadata, 'uc-application')
  const infra = packageByName(metadata, 'uc-infra')

  for (const packageMetadata of [engine, application, infra]) {
    if (featureItems(packageMetadata, 'default').includes('lan-compat')) {
      addProblem(
        problems,
        'compatibility gate',
        `${packageMetadata.name} enables lan-compat by default`
      )
    }
  }

  const engineLan = featureItems(engine, 'lan-compat')
  for (const required of [
    'dep:uc-mobile-proto',
    'uc-application/lan-compat',
    'uc-infra/lan-compat',
  ]) {
    if (!engineLan.includes(required)) {
      addProblem(problems, 'compatibility gate', `uc-engine/lan-compat is missing ${required}`)
    }
  }

  const protocol = normalDependency(application, 'uc-mobile-proto')
  if (!protocol?.optional) {
    addProblem(problems, 'compatibility gate', 'uc-application must keep uc-mobile-proto optional')
  }
  const networkInterface = normalDependency(infra, 'network-interface')
  if (!networkInterface?.optional) {
    addProblem(problems, 'compatibility gate', 'uc-infra must keep network-interface optional')
  }

  for (const consumerName of P2P_CONSUMERS) {
    const dependency = normalDependency(packageByName(metadata, consumerName), 'uc-engine')
    if (dependency?.features.includes('lan-compat')) {
      addProblem(
        problems,
        'compatibility gate',
        `${consumerName} must not enable uc-engine/lan-compat`
      )
    }
  }
  const webserverEngine = normalDependency(packageByName(metadata, 'uc-webserver'), 'uc-engine')
  if (!webserverEngine?.features.includes('lan-compat')) {
    addProblem(
      problems,
      'compatibility gate',
      'uc-webserver must enable uc-engine/lan-compat explicitly'
    )
  }

  const targetSignature =
    'pub(crate) fn initial_lan_target(view: Option<&MobileSyncSettingsSummary>)'
  if (!sources.mobileLanLifecycle.includes(targetSignature)) {
    addProblem(
      problems,
      'compatibility gate',
      'initial LAN state must be derived only from explicit mobile-sync settings'
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
    addProblem(
      problems,
      'compatibility gate',
      'engine event handling contains an automatic LAN fallback'
    )
  }

  return problems
}

function repositorySources() {
  return {
    uniffi: read('crates/uc-engine-uniffi/src/lib.rs'),
    ohos: read('crates/uc-ohos-napi/src/lib.rs'),
    iosPackaging: read('crates/uc-engine-uniffi/scripts/build-ios-xcframework.sh'),
    androidPackaging: read('crates/uc-engine-uniffi/scripts/build-android-aar.sh'),
    ohosPackaging: read('apps/ohos-probe/build-emulator.sh'),
    mobileLanLifecycle: read('apps/daemon/src/daemon/mobile_lan_lifecycle.rs'),
    engineEvents: read('apps/daemon/src/daemon/engine_events.rs'),
  }
}

function checkPlaintextScanner() {
  const problems = []
  const work = mkdtempSync(join(tmpdir(), 'uc-core-repository-check-'))
  const probe = join(work, 'probe.txt')
  const root = join(work, 'storage')
  const scanner = join(REPOSITORY_ROOT, 'scripts/security/scan-plaintext-probe.sh')
  const value = 'cross-repository-plaintext-probe-20260724'

  try {
    mkdirSync(root)
    writeFileSync(probe, value)
    writeFileSync(join(root, 'encrypted.db'), 'ciphertext-only')
    const clean = spawnSync('bash', [scanner, probe, root], { encoding: 'utf8' })
    if (clean.status !== 0) {
      addProblem(problems, 'persistence gate', 'plaintext scanner rejected a clean fixture')
    }

    writeFileSync(join(root, 'leak.db'), value)
    const leaking = spawnSync('bash', [scanner, probe, root], { encoding: 'utf8' })
    if (leaking.status === 0) {
      addProblem(problems, 'persistence gate', 'plaintext scanner accepted a leaking fixture')
    }
    const output = `${leaking.stdout}${leaking.stderr}`
    if (output.includes(value)) {
      addProblem(problems, 'persistence gate', 'plaintext scanner printed the probe value')
    }
  } finally {
    rmSync(work, { recursive: true, force: true })
  }

  const rootRules = read('AGENTS.md')
  if (!rootRules.includes('文件内容本体') || !rootRules.includes('严禁明文落库')) {
    addProblem(
      problems,
      'persistence gate',
      'repository rules must preserve encrypted persistence and the raw file-content exception'
    )
  }

  return problems
}

function collectProblems(metadata, sources, { includePlaintext = true } = {}) {
  return [
    ...checkCoreDependencies(metadata),
    ...checkPublicSurface(metadata),
    ...checkBindingProvenance(metadata, sources),
    ...checkLanIsolation(metadata, sources),
    ...(includePlaintext ? checkPlaintextScanner() : []),
  ]
}

function clone(value) {
  return structuredClone(value)
}

function expectRejected(name, mutate, metadata, sources) {
  const changedMetadata = clone(metadata)
  const changedSources = { ...sources }
  mutate(changedMetadata, changedSources)
  const problems = collectProblems(changedMetadata, changedSources, { includePlaintext: false })
  if (problems.length === 0) {
    throw new Error(`negative fixture was not rejected: ${name}`)
  }
  process.stdout.write(`OK negative fixture rejected: ${name}\n`)
}

function runNegativeFixtures(metadata, sources) {
  expectRejected(
    'reverse desktop dependency',
    changed => {
      packageByName(changed, 'uc-engine').dependencies.push({
        name: 'uc-platform',
        kind: null,
        path: join(REPOSITORY_ROOT, 'crates/uc-platform'),
        features: [],
        optional: false,
      })
    },
    metadata,
    sources
  )
  expectRejected(
    'binding version mismatch',
    changed => {
      packageByName(changed, 'uc-ohos-napi').version = '999.0.0'
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
  process.stdout.write('Core repository preflight passed\n')
}

main()
