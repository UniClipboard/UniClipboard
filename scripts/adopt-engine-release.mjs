#!/usr/bin/env node

import { Buffer } from 'node:buffer'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import process from 'node:process'

const ENGINE_REPOSITORY = 'UniClipboard/Engine'
const ENGINE_GIT_URL = `https://github.com/${ENGINE_REPOSITORY}.git`

function fail(message) {
  process.stderr.write(`Engine adoption failed: ${message}\n`)
  process.exit(1)
}

function readArg(name) {
  const index = process.argv.indexOf(name)
  if (index === -1) return undefined
  const value = process.argv[index + 1]
  if (!value) fail(`${name} requires a value`)
  return value
}

async function readBytes(path, url) {
  if (path) return readFileSync(resolve(path))
  const response = await fetch(url, { redirect: 'follow' })
  if (!response.ok) fail(`cannot download ${url}: HTTP ${response.status}`)
  return Buffer.from(await response.arrayBuffer())
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

const root = resolve(readArg('--root') ?? resolve(import.meta.dirname, '..'))
const version = readArg('--version')
const sourceCommit = readArg('--source-commit')
if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version ?? '')) {
  fail('version must be a v-prefixed semantic version')
}
if (!/^[0-9a-f]{40}$/.test(sourceCommit ?? '')) {
  fail('source commit must be a full lowercase Git commit')
}

const releaseBase = `https://github.com/${ENGINE_REPOSITORY}/releases/download/${version}`
const manifestBytes = await readBytes(readArg('--manifest'), `${releaseBase}/release-manifest.json`)
let manifest
try {
  manifest = JSON.parse(manifestBytes.toString('utf8'))
} catch (error) {
  fail(`release manifest is invalid JSON: ${String(error)}`)
}
if (manifest.schemaVersion !== 1) fail('unsupported release manifest schema')
if (manifest.release?.version !== version) fail('release version does not match the notification')
if (manifest.release?.commit !== sourceCommit) {
  fail('release source commit does not match the notification')
}

const sourceArtifact = manifest.artifacts?.find(
  candidate => candidate.name === 'UniClipboardEngine-source.tar.gz'
)
if (!sourceArtifact || !/^[0-9a-f]{64}$/.test(sourceArtifact.sha256 ?? '')) {
  fail('release manifest does not declare a valid source archive')
}
const sourceBytes = await readBytes(
  readArg('--source-archive'),
  `${releaseBase}/UniClipboardEngine-source.tar.gz`
)
if (sourceBytes.length !== sourceArtifact.size)
  fail('source archive size does not match the release')
if (sha256(sourceBytes) !== sourceArtifact.sha256) {
  fail('source archive checksum does not match the release')
}

const cargoPath = resolve(root, 'Cargo.toml')
let cargo = readFileSync(cargoPath, 'utf8')
let replacements = 0
for (const dependency of ['uc-engine', 'uc-observability-contract']) {
  const pattern = new RegExp(
    `^(${dependency.replace('-', '\\-')}\\s*=\\s*\\{[^\\n]*git\\s*=\\s*"${ENGINE_GIT_URL.replaceAll('.', '\\.')}"[^\\n]*rev\\s*=\\s*")[0-9a-f]{40}("[^\\n]*\\})$`,
    'm'
  )
  if (!pattern.test(cargo)) fail(`Cargo.toml does not contain the pinned ${dependency} dependency`)
  cargo = cargo.replace(pattern, `$1${sourceCommit}$2`)
  replacements += 1
}
if (replacements !== 2) fail('did not update every Engine dependency')
writeFileSync(cargoPath, cargo)

process.stdout.write(`Adopted Engine ${version} at ${sourceCommit}\n`)
