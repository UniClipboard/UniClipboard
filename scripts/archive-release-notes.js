#!/usr/bin/env node

/**
 * Archive Release Notes Script
 *
 * For each release this script:
 *   1. Writes the single-version archive `release-notes/v<version>.json` to R2
 *      with structure { version, channel, pub_date, notes_en, notes_zh }.
 *   2. Reads the channel index `release-notes/index/<channel>.json` from R2
 *      (empty if missing), inserts the new version, semver-sorts descending,
 *      deduplicates, and writes it back.
 *
 * Usage:
 *   node scripts/archive-release-notes.js \
 *     --version 0.11.0-alpha.6 \
 *     --channel alpha \
 *     --notes-file docs/changelog/0.11.0-alpha.6.md \
 *     --zh-notes-file docs/changelog/0.11.0-alpha.6.zh.md \
 *     [--pub-date 2026-05-22T10:30:00Z] \
 *     [--bucket uniclipboard-releases]
 *
 * Required env (for wrangler r2 calls):
 *   CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID
 */

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import semver from 'semver'

const DEFAULT_BUCKET = 'uniclipboard-releases'

function parseArgs(argv = process.argv.slice(2)) {
  const options = {
    version: null,
    channel: null,
    notesFile: null,
    zhNotesFile: null,
    pubDate: null,
    bucket: DEFAULT_BUCKET,
  }

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    const next = argv[i + 1]
    if (arg === '--version' && next) {
      options.version = next
      i += 1
    } else if (arg === '--channel' && next) {
      options.channel = next
      i += 1
    } else if (arg === '--notes-file' && next) {
      options.notesFile = next
      i += 1
    } else if (arg === '--zh-notes-file' && next) {
      options.zhNotesFile = next
      i += 1
    } else if (arg === '--pub-date' && next) {
      options.pubDate = next
      i += 1
    } else if (arg === '--bucket' && next) {
      options.bucket = next
      i += 1
    }
  }

  return options
}

function ensureRequired(options) {
  const required = ['version', 'channel', 'notesFile', 'zhNotesFile']
  const missing = required.filter(key => !options[key])
  if (missing.length > 0) {
    throw new Error(`Missing required options: ${missing.join(', ')}`)
  }
}

function normalizeVersion(version) {
  const stripped = String(version).replace(/^v/, '')
  const parsed = semver.parse(stripped)
  return parsed ? parsed.version : null
}

/**
 * Insert a new entry into the channel index, sort descending by semver,
 * and deduplicate (newer pub_date wins for the same version).
 *
 * Exported for unit testing.
 */
export function upsertVersionIntoIndex(index, newEntry) {
  const all = [...index.versions.filter(v => v.version !== newEntry.version), newEntry]
  all.sort((a, b) => {
    const na = normalizeVersion(a.version) ?? '0.0.0'
    const nb = normalizeVersion(b.version) ?? '0.0.0'
    return semver.rcompare(na, nb)
  })
  return {
    channel: index.channel,
    updated_at: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    versions: all,
  }
}

/**
 * Build the per-version archive JSON. Exported for testing.
 */
export function buildArchive({ version, channel, pubDate, notesEn, notesZh }) {
  return {
    version,
    channel,
    pub_date: pubDate,
    notes_en: notesEn,
    notes_zh: notesZh,
  }
}

function readNotes(filePath, label) {
  if (!fs.existsSync(filePath)) {
    process.stderr.write(`Warning: ${label} file not found at ${filePath}, using empty string\n`)
    return ''
  }
  return fs.readFileSync(filePath, 'utf8').trim()
}

function wrangler(args) {
  return execFileSync('wrangler', args, { stdio: ['ignore', 'pipe', 'pipe'] })
}

function r2GetText(bucket, key) {
  // wrangler r2 object get writes the body to --file or stdout when --pipe is set.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'archive-notes-'))
  const outPath = path.join(tmp, 'object')
  try {
    wrangler(['r2', 'object', 'get', `${bucket}/${key}`, '--file', outPath, '--remote'])
    return fs.readFileSync(outPath, 'utf8')
  } catch (err) {
    // wrangler returns non-zero on 404; treat as "missing"
    const stderr = String(err?.stderr ?? '')
    if (stderr.includes('not found') || stderr.includes('NoSuchKey') || stderr.includes('404')) {
      return null
    }
    throw err
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true })
  }
}

function r2PutJson(bucket, key, body) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'archive-notes-'))
  const filePath = path.join(tmp, 'object.json')
  fs.writeFileSync(filePath, body, 'utf8')
  try {
    wrangler([
      'r2',
      'object',
      'put',
      `${bucket}/${key}`,
      '--file',
      filePath,
      '--content-type',
      'application/json',
      '--remote',
    ])
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true })
  }
}

function main() {
  const options = parseArgs()
  ensureRequired(options)

  const normalized = normalizeVersion(options.version)
  if (!normalized) {
    throw new Error(`Invalid semver version: ${options.version}`)
  }

  const pubDate = options.pubDate || new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
  const notesEn = readNotes(options.notesFile, 'English notes')
  const notesZh = readNotes(options.zhNotesFile, 'Chinese notes')

  const archive = buildArchive({
    version: normalized,
    channel: options.channel,
    pubDate,
    notesEn,
    notesZh,
  })

  const archiveKey = `release-notes/v${normalized}.json`
  process.stderr.write(`Uploading ${archiveKey} → ${options.bucket}\n`)
  r2PutJson(options.bucket, archiveKey, JSON.stringify(archive, null, 2))

  // Read existing index (empty if missing)
  const indexKey = `release-notes/index/${options.channel}.json`
  process.stderr.write(`Fetching ${indexKey}…\n`)
  const indexText = r2GetText(options.bucket, indexKey)
  const existingIndex = indexText
    ? JSON.parse(indexText)
    : { channel: options.channel, updated_at: pubDate, versions: [] }

  const nextIndex = upsertVersionIntoIndex(existingIndex, {
    version: normalized,
    pub_date: pubDate,
  })

  process.stderr.write(
    `Index now has ${nextIndex.versions.length} versions; uploading ${indexKey}\n`
  )
  r2PutJson(options.bucket, indexKey, JSON.stringify(nextIndex, null, 2))

  process.stderr.write('Done.\n')
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main()
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err))
    process.exit(1)
  }
}
