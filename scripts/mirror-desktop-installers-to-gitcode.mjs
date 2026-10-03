#!/usr/bin/env node

// Copies the already built, signed desktop installers that were registered
// with FlareRelease to a GitCode Release and registers each verified copy
// with FlareRelease's PUT /api/mirrors contract. It never rebuilds or
// re-signs: the bytes uploaded are the bytes already on disk, and they are
// downloaded back anonymously and compared by size and SHA-256 before
// FlareRelease is told any mirror is ready. GitHub and R2 remain the
// authoritative sources; this step only ever adds an optional copy.
//
// Only artifacts present in the FlareRelease registration payload (the
// updater-selected archive per platform: macOS .app.tar.gz, Linux
// .AppImage(.tar.gz), Windows .nsis.zip/.exe) can be mirrored, because
// PUT /api/mirrors only accepts a mirror for an artifact FlareRelease
// already has a registered (product, version, filename, size, sha256) for.
// Other installer formats produced by the same release (.dmg, .deb, .rpm,
// the Windows portable zip) are not registered with FlareRelease today and
// are therefore skipped — see docs/release-workflow.md "GitCode mirror".
//
// Mirrors every artifact independently: one artifact failing to mirror does
// not stop the others, matching the per-platform granularity of the
// underlying contract. Mirrors a single GitCode Release per tag shared by
// all platforms, same as the GitHub Release.

import { createHash } from 'node:crypto'
import { createReadStream, mkdirSync, readFileSync, writeFileSync, appendFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { Readable } from 'node:stream'

const DEFAULT_API_BASE = 'https://api.gitcode.com/api/v5'
const DEFAULT_ADMIN_URL = 'https://release-admin.uniclipboard.app'
// Keep in sync with FlareRelease src/domain/mirror.ts (the server enforces it too).
const ALLOWED_HOSTS = ['gitcode.com', 'atomgit.com']
const MAX_REDIRECTS = 5

class MirrorError extends Error {}

function argValue(name, fallback) {
  const index = process.argv.indexOf(name)
  if (index === -1) return fallback
  const value = process.argv[index + 1]
  if (value === undefined || value.startsWith('--'))
    throw new MirrorError(`${name} requires a value`)
  return value
}

const hasFlag = name => process.argv.includes(name)

function numberArg(name, fallback) {
  const raw = argValue(name, undefined)
  if (raw === undefined) return fallback
  const value = Number(raw)
  if (!Number.isFinite(value) || value < 0)
    throw new MirrorError(`${name} must be a non-negative number`)
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

async function sha256File(path) {
  const hash = createHash('sha256')
  let size = 0
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk)
    size += chunk.length
  }
  return { sha256: hash.digest('hex'), size }
}

const sleep = ms => new Promise(resolveSleep => setTimeout(resolveSleep, ms))

// One deadline for the whole run, shared by every artifact. A workflow job
// timeout would fail the job even for a non-blocking step, so the script has
// to stop itself, earlier, regardless of how many artifacts remain.
let deadlineAt = Number.POSITIVE_INFINITY
let deadlineMs = 0
const remaining = () => Math.max(1, deadlineAt - Date.now())
const pastDeadline = () => Date.now() >= deadlineAt
const deadlineError = detail => new MirrorError(`deadline of ${deadlineMs} ms exceeded (${detail})`)

async function withRetries(label, attempts, delayMs, task) {
  let last
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    if (pastDeadline()) throw deadlineError(label)
    try {
      return await task(attempt)
    } catch (error) {
      last = error
      if (error?.permanent) break
      if (pastDeadline())
        throw deadlineError(`${label}: ${error instanceof Error ? error.message : String(error)}`)
      if (attempt < attempts) await sleep(Math.min(delayMs * attempt, remaining()))
    }
  }
  const message = last instanceof Error ? last.message : String(last)
  throw new MirrorError(`${label} failed after ${attempts} attempt(s): ${message}`)
}

function timeoutSignal(ms) {
  return AbortSignal.any([AbortSignal.timeout(ms), AbortSignal.timeout(remaining())])
}

function assertSuccess(response, label, body) {
  if (response.ok) return
  const error = new Error(
    `${label} returned HTTP ${response.status}${body ? `: ${String(body).slice(0, 300)}` : ''}`
  )
  // Client errors other than throttling will not change on retry.
  if (
    response.status >= 400 &&
    response.status < 500 &&
    response.status !== 429 &&
    response.status !== 408
  ) {
    error.permanent = true
  }
  throw error
}

function api(apiBase, token, apiTimeoutMs) {
  return async (method, path, { query = {}, body } = {}) => {
    const url = new URL(`${apiBase}${path}`)
    for (const [key, value] of Object.entries(query)) url.searchParams.set(key, value)
    url.searchParams.set('access_token', token)
    const response = await fetch(url, {
      method,
      headers: body ? { 'content-type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
      signal: timeoutSignal(apiTimeoutMs),
    })
    const text = await response.text()
    return { response, text, json: () => (text ? JSON.parse(text) : null) }
  }
}

// Download anonymously: no token, no cookies, redirects followed by hand so
// every hop can be checked and recorded.
async function download(startUrl, { allowLocal, transferTimeoutMs }) {
  const chain = []
  let url = startUrl
  for (let hop = 0; hop <= MAX_REDIRECTS; hop += 1) {
    const target = new URL(url)
    if (!(target.protocol === 'https:' || (allowLocal && target.protocol === 'http:'))) {
      throw new MirrorError(
        `Download redirected to a non-https address: ${target.protocol}//${target.host}`
      )
    }
    chain.push(`${target.origin}${target.pathname}`)
    const response = await fetch(url, {
      redirect: 'manual',
      signal: timeoutSignal(transferTimeoutMs),
    })
    if (response.status >= 300 && response.status < 400) {
      const location = response.headers.get('location')
      if (!location) throw new MirrorError(`HTTP ${response.status} without a Location header`)
      url = new URL(location, url).href
      await response.body?.cancel()
      continue
    }
    if (!response.ok) {
      await response.body?.cancel()
      throw new MirrorError(`Anonymous download returned HTTP ${response.status}`)
    }
    const hash = createHash('sha256')
    let received = 0
    for await (const chunk of Readable.fromWeb(response.body)) {
      hash.update(chunk)
      received += chunk.length
    }
    return { chain, size: received, sha256: hash.digest('hex'), finalUrl: url }
  }
  throw new MirrorError('Too many redirects')
}

// Mirrors one artifact: ensures a GitCode release exists for the tag, reuses
// or uploads the file, verifies the anonymous download, then registers the
// mirror with FlareRelease. Returns a provenance record; never throws for an
// expected failure — the caller decides whether to keep going.
async function mirrorArtifact(filePath, config) {
  const {
    tag,
    version,
    prerelease,
    source,
    token,
    owner,
    repo,
    apiBase,
    adminUrl,
    accessId,
    accessSecret,
    targetCommitish,
    allowLocal,
    attempts,
    delayMs,
    pollIntervalMs,
    pollAttempts,
    apiTimeoutMs,
    transferTimeoutMs,
    log,
  } = config
  const secrets = [token, accessSecret, accessId]
  const filename = basename(filePath)
  const { sha256, size } = await sha256File(filePath)
  const record = { tag, version, filename, size, sha256, provider: 'gitcode', owner, repo, source }
  log(`${filename}: ${size} bytes, sha256 ${sha256}`)

  const call = api(apiBase, token, apiTimeoutMs)
  const releasePath = `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases`
  const tagPath = `${releasePath}/${encodeURIComponent(tag)}`

  const fetchRelease = () =>
    withRetries('Reading the GitCode release', attempts, delayMs, async () => {
      const { response, text, json } = await call(
        'GET',
        `${releasePath}/tags/${encodeURIComponent(tag)}`
      )
      if (response.status === 404) return null
      assertSuccess(response, 'Reading the GitCode release', redact(text, secrets))
      return json()
    })
  let release = await fetchRelease()
  if (!release) {
    log(`Creating GitCode release ${tag}`)
    await withRetries('Creating the GitCode release', attempts, delayMs, async () => {
      const { response, text } = await call('POST', releasePath, {
        body: {
          tag_name: tag,
          name: tag,
          body: `Mirror of the GitHub release ${tag}.`,
          target_commitish: targetCommitish,
          release_status: prerelease ? 'pre' : 'latest',
        },
      })
      assertSuccess(response, 'Creating the GitCode release', redact(text, secrets))
    })
    release = await fetchRelease()
    if (!release) throw new MirrorError('The GitCode release is not readable after it was created')
  }

  const findAsset = value => (value?.assets ?? []).find(asset => asset.name === filename)
  const apiDownloadUrl = `${apiBase}${tagPath}/attach_files/${encodeURIComponent(filename)}/download`

  const candidates = value => {
    const found = findAsset(value)
    const urls = []
    if (
      found?.browser_download_url &&
      isRegistrableUrl(found.browser_download_url, { allowLocal })
    ) {
      urls.push(found.browser_download_url)
    }
    if (isRegistrableUrl(apiDownloadUrl, { allowLocal })) urls.push(apiDownloadUrl)
    return urls
  }

  const verify = async value => {
    const failures = []
    for (const candidate of candidates(value)) {
      try {
        const result = await download(candidate, { allowLocal, transferTimeoutMs })
        if (result.size !== size || result.sha256 !== sha256) {
          failures.push(
            `${new URL(candidate).host}: downloaded ${result.size} bytes sha256 ${result.sha256}, expected ${size} bytes sha256 ${sha256}`
          )
          continue
        }
        return { downloadUrl: candidate, chain: result.chain }
      } catch (error) {
        failures.push(
          `${new URL(candidate).host}: ${error instanceof Error ? error.message : String(error)}`
        )
      }
    }
    return { failures }
  }

  // Reuse an identical upload, refuse to overwrite a different one, else upload.
  let verified
  let reused = false
  if (findAsset(release)) {
    log(`GitCode already has ${filename}; checking that it is identical`)
    verified = await verify(release)
    if (!verified.downloadUrl) {
      throw new MirrorError(
        `GitCode already has ${filename} but it cannot be verified as identical (${verified.failures.join('; ')}); ` +
          'not overwriting or deleting it'
      )
    }
    reused = true
  } else {
    await withRetries('Uploading to GitCode', attempts, delayMs, async attempt => {
      log(`Uploading ${filename} (attempt ${attempt}/${attempts})`)
      const { response, text, json } = await call('GET', `${tagPath}/upload_url`, {
        query: { file_name: filename },
      })
      assertSuccess(response, 'Requesting the upload address', redact(text, secrets))
      const target = json()
      if (!target?.url) throw new Error('The upload address response has no url')
      const headers = { ...(target.headers ?? {}), 'content-length': String(size) }
      const put = await fetch(target.url, {
        method: 'PUT',
        headers,
        body: Readable.toWeb(createReadStream(filePath)),
        duplex: 'half',
        signal: timeoutSignal(transferTimeoutMs),
      })
      const putText = await put.text()
      assertSuccess(put, 'Uploading the file', redact(putText, secrets))
    })
    // The upload is registered asynchronously through GitCode's callback.
    let refreshed = null
    for (let poll = 0; poll < pollAttempts && !pastDeadline(); poll += 1) {
      refreshed = await fetchRelease()
      if (findAsset(refreshed)) break
      await sleep(Math.min(pollIntervalMs, remaining()))
    }
    if (!findAsset(refreshed)) {
      throw new MirrorError('The uploaded file did not appear on the GitCode release')
    }
    verified = await verify(refreshed)
    if (!verified.downloadUrl) {
      throw new MirrorError(
        `The uploaded file failed the anonymous download check (${verified.failures.join('; ')})`
      )
    }
  }
  Object.assign(record, {
    reused,
    downloadUrl: verified.downloadUrl,
    redirectChain: verified.chain,
  })

  // Informational: whether the mirror can resume downloads.
  try {
    const probe = await fetch(verified.downloadUrl, {
      headers: { range: 'bytes=0-0' },
      redirect: 'follow',
      signal: timeoutSignal(apiTimeoutMs),
    })
    record.acceptsRange = probe.status === 206
    await probe.body?.cancel()
  } catch {
    record.acceptsRange = null
  }

  // Tell FlareRelease. It checks size and sha256 against the artifact it
  // already has registered for (product, version, filename) and only then
  // marks the mirror ready.
  const registration = {
    product: 'desktop',
    version,
    filename,
    provider: 'gitcode',
    downloadUrl: verified.downloadUrl,
    size,
    sha256,
    source,
  }
  const result = await withRetries(
    'Registering the mirror with FlareRelease',
    attempts,
    delayMs,
    async () => {
      const response = await fetch(`${adminUrl}/api/mirrors`, {
        method: 'PUT',
        headers: {
          'content-type': 'application/json',
          'CF-Access-Client-Id': accessId,
          'CF-Access-Client-Secret': accessSecret,
        },
        body: JSON.stringify(registration),
        signal: timeoutSignal(apiTimeoutMs),
      })
      const text = await response.text()
      assertSuccess(response, 'FlareRelease', redact(text, secrets))
      return JSON.parse(text)
    }
  )
  if (result?.data?.state !== 'ready') {
    throw new MirrorError(
      `FlareRelease did not mark the mirror ready: ${JSON.stringify(result).slice(0, 300)}`
    )
  }
  Object.assign(record, { status: 'mirrored', registeredAt: new Date().toISOString() })
  return record
}

export async function mirrorInstallers(config) {
  deadlineMs = config.deadlineMs
  deadlineAt = Date.now() + config.deadlineMs
  const results = []
  for (const filePath of config.files) {
    const filename = basename(filePath)
    if (pastDeadline()) {
      results.push({
        filename,
        status: 'skipped',
        error: `deadline of ${deadlineMs} ms exceeded before this artifact started`,
      })
      continue
    }
    try {
      results.push(await mirrorArtifact(filePath, config))
    } catch (error) {
      const raw = error instanceof Error ? error.message : String(error)
      results.push({
        tag: config.tag,
        version: config.version,
        filename,
        status: 'failed',
        error: raw,
      })
    }
  }
  return results
}

function writeOutputs(records, provenancePath, secrets) {
  const text = redact(`${JSON.stringify(records, null, 2)}\n`, secrets)
  if (provenancePath) {
    mkdirSync(dirname(resolve(provenancePath)), { recursive: true })
    writeFileSync(resolve(provenancePath), text)
  }
  const summary = process.env.GITHUB_STEP_SUMMARY
  if (summary) {
    const lines = ['### GitCode mirror (desktop installers)', '']
    for (const record of records) {
      lines.push(
        `- \`${record.filename}\`: **${record.status}**${record.size ? ` (${record.size} bytes)` : ''}`
      )
      if (record.downloadUrl) lines.push(`  - ${record.downloadUrl}`)
      if (record.error) lines.push(`  - ${record.error}`)
    }
    appendFileSync(summary, `${redact(lines.join('\n'), secrets)}\n`)
  }
}

function readRegisteredFiles(registrationPath, artifactsDir) {
  const registration = JSON.parse(readFileSync(resolve(registrationPath), 'utf8'))
  const artifacts = Array.isArray(registration.artifacts) ? registration.artifacts : []
  if (artifacts.length === 0) {
    throw new MirrorError(`${registrationPath} has no artifacts to mirror`)
  }
  return {
    version: registration.version,
    tagName: registration.tagName,
    prerelease: Boolean(registration.prerelease),
    files: artifacts.map(artifact => resolve(join(artifactsDir, artifact.filename))),
  }
}

async function main() {
  const log = message => console.log(redact(message, []))
  const missingConfig = argValue('--missing-config', 'fail')
  const allowLocal = hasFlag('--allow-local-http')
  const provenancePath = argValue('--provenance', undefined)
  const registrationPath = argValue('--registration', undefined)
  const artifactsDir = argValue('--artifacts-dir', undefined)
  if (!registrationPath || !artifactsDir) {
    throw new MirrorError('--registration and --artifacts-dir are required')
  }
  const { version, tagName, prerelease, files } = readRegisteredFiles(
    registrationPath,
    artifactsDir
  )
  const tag = argValue('--tag', tagName)
  const source = argValue('--source', 'github-actions')

  const env = process.env
  const config = {
    files,
    tag,
    version,
    prerelease,
    source,
    token: env.GITCODE_TOKEN,
    owner: env.GITCODE_OWNER,
    repo: env.GITCODE_REPO,
    apiBase: (env.GITCODE_API_BASE || DEFAULT_API_BASE).replace(/\/$/, ''),
    adminUrl: (env.FLARE_RELEASE_ADMIN_URL || DEFAULT_ADMIN_URL).replace(/\/$/, ''),
    accessId: env.FLARE_RELEASE_ACCESS_CLIENT_ID,
    accessSecret: env.FLARE_RELEASE_ACCESS_CLIENT_SECRET,
    targetCommitish: env.GITCODE_TARGET_COMMITISH || 'main',
    allowLocal,
    attempts: numberArg('--attempts', 3),
    delayMs: numberArg('--retry-delay-ms', 5000),
    pollIntervalMs: numberArg('--poll-interval-ms', 3000),
    pollAttempts: numberArg('--poll-attempts', 20),
    apiTimeoutMs: numberArg('--api-timeout-ms', 60_000),
    // One bounded attempt per transfer; the workflow also has a job timeout.
    transferTimeoutMs: numberArg('--transfer-timeout-ms', 600_000),
    // Must stay below the workflow step timeout, shared across all artifacts.
    deadlineMs: numberArg('--deadline-ms', 1_080_000),
    log,
  }
  const secrets = [config.token, config.accessSecret, config.accessId]

  const missing = [
    ['GITCODE_TOKEN', config.token],
    ['GITCODE_OWNER', config.owner],
    ['GITCODE_REPO', config.repo],
    ['FLARE_RELEASE_ACCESS_CLIENT_ID', config.accessId],
    ['FLARE_RELEASE_ACCESS_CLIENT_SECRET', config.accessSecret],
  ]
    .filter(([, value]) => !value)
    .map(([name]) => name)
  if (missing.length > 0) {
    const reason = `GitCode mirroring is not configured; missing ${missing.join(', ')}`
    const records = files.map(filePath => ({
      tag,
      version,
      filename: basename(filePath),
      status: 'skipped',
      error: reason,
    }))
    console.log(`::warning title=GitCode mirror skipped::${reason}`)
    writeOutputs(records, provenancePath, secrets)
    process.exit(missingConfig === 'skip' ? 0 : 1)
  }

  if (!isRegistrableUrl(`${config.apiBase}/`, { allowLocal })) {
    throw new MirrorError('GITCODE_API_BASE must be an https GitCode or AtomGit address')
  }
  if (!allowLocal && !config.adminUrl.startsWith('https://')) {
    throw new MirrorError('FLARE_RELEASE_ADMIN_URL must be https')
  }

  const records = await mirrorInstallers(config)
  writeOutputs(records, provenancePath, secrets)

  const failed = records.filter(record => record.status === 'failed' || record.status === 'skipped')
  for (const record of records) {
    if (record.status === 'mirrored') {
      console.log(redact(`Mirrored ${record.filename} to ${record.downloadUrl}`, secrets))
    } else {
      console.log(
        `::warning title=GitCode mirror failed::${record.filename}: ${redact(record.error ?? 'unknown error', secrets)}`
      )
    }
  }
  if (failed.length > 0) process.exit(1)
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch(error => {
    const reason = redact(error instanceof Error ? error.message : String(error), [
      process.env.GITCODE_TOKEN,
      process.env.FLARE_RELEASE_ACCESS_CLIENT_SECRET,
    ])
    console.log(`::warning title=GitCode mirror failed::${reason}`)
    console.error(`mirror-desktop-installers-to-gitcode failed: ${reason}`)
    process.exit(1)
  })
}
