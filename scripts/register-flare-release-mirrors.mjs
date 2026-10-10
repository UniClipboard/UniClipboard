#!/usr/bin/env node
// Registers GitCode mirror copies with FlareRelease after the
// mkdir700/gitcode-release-mirror Action has uploaded and verified them.
//
// The Action owns "put these bytes on GitCode and prove it"; this script owns
// the Desktop-specific follow-up, PUT /api/mirrors, and runs on the CI runner so
// the FlareRelease Access credentials never leave GitHub.
//
// Inputs: the Action's `receipt` JSON and the registration.json that was built
// from the same artifacts. A file is registered only when the receipt says it is
// `mirrored` or `reused`, its digest equals the registered artifact's, and its
// download address is a plain https GitCode/AtomGit address. FlareRelease then
// checks size and sha256 again against what it recorded for the release.
//
// Usage:
//   node scripts/register-flare-release-mirrors.mjs --receipt receipt.json \
//     --registration flare-release/registration.json [--source <text>] \
//     [--provenance out.json] [--attempts 3] [--retry-delay-ms 5000]
// Environment: FLARE_RELEASE_ACCESS_CLIENT_ID, FLARE_RELEASE_ACCESS_CLIENT_SECRET,
// optional FLARE_RELEASE_ADMIN_URL.

import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'

const DEFAULT_ADMIN_URL = 'https://release-admin.uniclipboard.app'
// Keep in sync with FlareRelease src/domain/mirror.ts (the server enforces it too).
const ALLOWED_HOSTS = ['gitcode.com', 'atomgit.com']
const SHA256 = /^[0-9a-f]{64}$/

class RegistrationError extends Error {}

function argValue(name, fallback) {
  const index = process.argv.indexOf(name)
  if (index === -1) return fallback
  const value = process.argv[index + 1]
  if (value === undefined || value.startsWith('--')) {
    throw new RegistrationError(`${name} requires a value`)
  }
  return value
}

function numberArg(name, fallback) {
  const value = Number(argValue(name, String(fallback)))
  if (!Number.isFinite(value) || value < 0) throw new RegistrationError(`${name} must be a number`)
  return value
}

export function isRegistrableUrl(value, { allowLocal = false } = {}) {
  let url
  try {
    url = new URL(value)
  } catch {
    return false
  }
  if (allowLocal && url.protocol === 'http:' && ['127.0.0.1', 'localhost'].includes(url.hostname)) {
    return true
  }
  if (url.protocol !== 'https:' || url.username || url.password || url.port) return false
  if (url.search || url.hash) return false
  return ALLOWED_HOSTS.some(host => url.hostname === host || url.hostname.endsWith(`.${host}`))
}

function redact(text, secrets) {
  let output = String(text).replace(/access_token=[^&\s"']+/g, 'access_token=***')
  for (const secret of secrets) {
    if (secret) output = output.split(secret).join('***')
  }
  return output
}

const sleep = ms => new Promise(resolveSleep => setTimeout(resolveSleep, ms))

function readJson(path, what) {
  try {
    return JSON.parse(readFileSync(resolve(path), 'utf8'))
  } catch (error) {
    throw new RegistrationError(`cannot read ${what} ${path}: ${error.message}`)
  }
}

// A 4xx other than 408/429 means FlareRelease understood and refused the
// request; repeating it cannot help.
function isPermanent(status) {
  return status >= 400 && status < 500 && status !== 408 && status !== 429
}

async function putMirror(config, body) {
  let last
  for (let attempt = 1; attempt <= config.attempts; attempt += 1) {
    try {
      const response = await fetch(`${config.adminUrl}/api/mirrors`, {
        method: 'PUT',
        headers: {
          'content-type': 'application/json',
          'CF-Access-Client-Id': config.accessId,
          'CF-Access-Client-Secret': config.accessSecret,
        },
        body: JSON.stringify(body),
        redirect: 'error',
        signal: AbortSignal.timeout(config.apiTimeoutMs),
      })
      const text = await response.text()
      if (!response.ok) {
        const error = new RegistrationError(
          `FlareRelease returned HTTP ${response.status}: ${text.slice(0, 300)}`
        )
        error.permanent = isPermanent(response.status)
        throw error
      }
      const parsed = JSON.parse(text)
      if (parsed?.data?.state !== 'ready') {
        const error = new RegistrationError(
          `FlareRelease did not mark the mirror ready: ${text.slice(0, 300)}`
        )
        error.permanent = true
        throw error
      }
      return parsed
    } catch (error) {
      last = error
      if (error?.permanent || attempt === config.attempts) break
      await sleep(config.delayMs * attempt)
    }
  }
  throw last
}

export async function registerMirrors(config) {
  const registered = new Map(config.artifacts.map(artifact => [artifact.filename, artifact]))
  const receiptFiles = new Map(config.receiptFiles.map(file => [file.name, file]))
  const records = []

  for (const [filename, artifact] of registered) {
    const entry = receiptFiles.get(filename)
    const base = { filename }
    if (!entry) {
      records.push({ ...base, status: 'failed', error: 'the receipt has no entry for this file' })
      continue
    }
    if (entry.status !== 'mirrored' && entry.status !== 'reused') {
      records.push({
        ...base,
        status: 'failed',
        error: `not registered: the mirror step reported ${entry.status}${entry.stage ? ` at ${entry.stage}` : ''}${entry.error ? `: ${entry.error}` : ''}`,
      })
      continue
    }
    if (!SHA256.test(String(entry.sha256)) || entry.sha256 !== artifact.sha256) {
      records.push({
        ...base,
        status: 'failed',
        error: 'not registered: the mirrored digest differs from the registered artifact',
      })
      continue
    }
    if (!Number.isSafeInteger(entry.size) || entry.size <= 0) {
      records.push({ ...base, status: 'failed', error: 'not registered: the receipt has no size' })
      continue
    }
    if (!isRegistrableUrl(entry.downloadUrl, { allowLocal: config.allowLocal })) {
      records.push({
        ...base,
        status: 'failed',
        error: 'not registered: the download address is not a plain https GitCode/AtomGit address',
      })
      continue
    }
    try {
      await putMirror(config, {
        product: 'desktop',
        version: config.version,
        filename,
        provider: 'gitcode',
        downloadUrl: entry.downloadUrl,
        size: entry.size,
        sha256: entry.sha256,
        source: config.source,
      })
      records.push({
        ...base,
        status: 'registered',
        mirror: entry.status,
        downloadUrl: entry.downloadUrl,
        size: entry.size,
        sha256: entry.sha256,
      })
    } catch (error) {
      records.push({
        ...base,
        status: 'failed',
        error: `registering with FlareRelease failed: ${error instanceof Error ? error.message : String(error)}`,
      })
    }
  }
  return records
}

function writeOutputs(records, provenancePath, secrets) {
  const text = redact(`${JSON.stringify(records, null, 2)}\n`, secrets)
  if (provenancePath) {
    mkdirSync(dirname(resolve(provenancePath)), { recursive: true })
    writeFileSync(resolve(provenancePath), text)
  }
  const summary = process.env.GITHUB_STEP_SUMMARY
  if (summary) {
    const lines = ['### FlareRelease mirror registration (desktop)', '']
    for (const record of records) {
      lines.push(`- \`${record.filename}\`: **${record.status}**`)
      if (record.error) lines.push(`  - ${record.error}`)
    }
    appendFileSync(summary, `${redact(lines.join('\n'), secrets)}\n`)
  }
}

async function main() {
  const receipt = readJson(argValue('--receipt', undefined) ?? '', 'receipt')
  const registration = readJson(argValue('--registration', undefined) ?? '', 'registration')
  const receiptFiles = Array.isArray(receipt?.files) ? receipt.files : []
  const artifacts = Array.isArray(registration?.artifacts) ? registration.artifacts : []
  if (receiptFiles.length === 0) throw new RegistrationError('the receipt lists no files')
  if (artifacts.length === 0) throw new RegistrationError('the registration lists no artifacts')

  const env = process.env
  const config = {
    version: registration.version,
    artifacts,
    receiptFiles,
    source: argValue('--source', 'github-actions'),
    adminUrl: (env.FLARE_RELEASE_ADMIN_URL || DEFAULT_ADMIN_URL).replace(/\/$/, ''),
    accessId: env.FLARE_RELEASE_ACCESS_CLIENT_ID,
    accessSecret: env.FLARE_RELEASE_ACCESS_CLIENT_SECRET,
    allowLocal: process.argv.includes('--allow-local-http'),
    attempts: numberArg('--attempts', 3),
    delayMs: numberArg('--retry-delay-ms', 5000),
    apiTimeoutMs: numberArg('--api-timeout-ms', 60_000),
  }
  const secrets = [config.accessSecret, config.accessId]

  const missing = [
    ['FLARE_RELEASE_ACCESS_CLIENT_ID', config.accessId],
    ['FLARE_RELEASE_ACCESS_CLIENT_SECRET', config.accessSecret],
  ]
    .filter(([, value]) => !value)
    .map(([name]) => name)
  if (missing.length > 0) {
    throw new RegistrationError(`FlareRelease is not configured; missing ${missing.join(', ')}`)
  }
  if (!config.allowLocal && !config.adminUrl.startsWith('https://')) {
    throw new RegistrationError('FLARE_RELEASE_ADMIN_URL must be https')
  }

  const records = await registerMirrors(config)
  writeOutputs(records, argValue('--provenance', undefined), secrets)

  let failed = 0
  for (const record of records) {
    if (record.status === 'registered') {
      console.log(`Registered ${record.filename} (${record.mirror}) -> ${record.downloadUrl}`)
    } else {
      failed += 1
      console.log(
        `::warning title=FlareRelease mirror not registered::${record.filename}: ${redact(record.error ?? 'unknown error', secrets)}`
      )
    }
  }
  if (failed > 0) process.exit(1)
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch(error => {
    const reason = redact(error instanceof Error ? error.message : String(error), [
      process.env.FLARE_RELEASE_ACCESS_CLIENT_ID,
      process.env.FLARE_RELEASE_ACCESS_CLIENT_SECRET,
    ])
    console.error(`register-flare-release-mirrors failed: ${reason}`)
    process.exit(1)
  })
}
