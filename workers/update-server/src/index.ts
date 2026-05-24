/**
 * UniClipboard Update Server
 *
 * Cloudflare Worker that serves update manifests and binary artifacts from R2.
 *
 * Routes:
 *   GET /{channel}.json                          → Update manifest (60s cache)
 *   GET /{channel}.json?from={version}           → Manifest with merged notes for (from, latest], capped at 5 versions
 *   GET /release-notes/v{version}.json           → Single-version archived release notes
 *   GET /release-notes/{channel}.json            → Channel version index
 *   GET /artifacts/v{ver}/{file}                 → Binary download (24h cache, immutable)
 *   GET /health                                  → Health check
 */

import { mergeNotes } from './merge'
import type { Manifest, ReleaseNotesArchive, VersionIndex } from './types'

interface Env {
  RELEASES_BUCKET: R2Bucket
}

const VALID_CHANNELS = new Set(['stable', 'alpha', 'beta', 'rc'])

const CORS_HEADERS: Record<string, string> = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, HEAD, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type',
}

function jsonResponse(
  body: unknown,
  status: number,
  extraHeaders?: Record<string, string>
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'Content-Type': 'application/json',
      ...CORS_HEADERS,
      ...extraHeaders,
    },
  })
}

function r2HeadersToResponse(
  object: R2Object,
  extraHeaders?: Record<string, string>
): Record<string, string> {
  const headers: Record<string, string> = {
    ...CORS_HEADERS,
    ...extraHeaders,
  }

  if (object.httpEtag) {
    headers['ETag'] = object.httpEtag
  }

  if (object.size !== undefined) {
    headers['Content-Length'] = object.size.toString()
  }

  if (object.httpMetadata?.contentType) {
    headers['Content-Type'] = object.httpMetadata.contentType
  }

  return headers
}

async function getJsonFromR2<T>(env: Env, key: string): Promise<T | null> {
  const object = await env.RELEASES_BUCKET.get(key)
  if (!object) return null
  const text = await object.text()
  try {
    return JSON.parse(text) as T
  } catch (err) {
    console.warn(`Failed to parse JSON at ${key}:`, err)
    return null
  }
}

async function handleChannelManifest(
  request: Request,
  channel: string,
  fromVersion: string | null,
  env: Env,
  ctx: ExecutionContext
): Promise<Response> {
  if (!VALID_CHANNELS.has(channel)) {
    return jsonResponse({ error: `Invalid channel: ${channel}` }, 400)
  }

  // No `?from=`: passthrough R2 object. Cloudflare's edge cache handles
  // Cache-Control automatically for ambiguity-free URLs — no manual cache.put needed.
  if (!fromVersion) {
    const key = `manifests/${channel}.json`
    const object = await env.RELEASES_BUCKET.get(key)
    if (!object) {
      return jsonResponse({ error: `Manifest not found for channel: ${channel}` }, 404)
    }
    const headers = r2HeadersToResponse(object, {
      'Content-Type': 'application/json',
      'Cache-Control': 'public, max-age=60',
    })
    return new Response(object.body, { status: 200, headers })
  }

  // With `?from=`: synthesize a manifest with merged notes.
  // Cloudflare's default cache key excludes query strings, so different `from`
  // values would collide on the same cache entry. Manage cache explicitly with
  // the full URL as key.
  const cache = caches.default
  const cacheKey = new Request(request.url, { method: 'GET' })
  const cached = await cache.match(cacheKey)
  if (cached) {
    return cached
  }

  const latestManifest = await getJsonFromR2<Manifest>(env, `manifests/${channel}.json`)
  if (!latestManifest) {
    return jsonResponse({ error: `Manifest not found for channel: ${channel}` }, 404)
  }

  const index = await getJsonFromR2<VersionIndex>(env, `release-notes/index/${channel}.json`)
  // If index is missing (e.g. first deploy before backfill), degrade to single-version notes.
  if (!index) {
    console.warn(`No release-notes index for channel=${channel}; serving single-version notes`)
    const response = jsonResponse(latestManifest, 200, {
      'Cache-Control': 'public, max-age=60',
    })
    ctx.waitUntil(cache.put(cacheKey, response.clone()))
    return response
  }

  const result = await mergeNotes(latestManifest, index, fromVersion, async version => {
    return await getJsonFromR2<ReleaseNotesArchive>(env, `release-notes/v${version}.json`)
  })

  console.log(
    `merge: channel=${channel} from=${fromVersion} merged=${result.mergedCount} truncated=${result.truncated} omitted=${result.omittedCount}`
  )

  const response = jsonResponse(result.manifest, 200, {
    'Cache-Control': 'public, max-age=60',
  })
  ctx.waitUntil(cache.put(cacheKey, response.clone()))
  return response
}

async function handleReleaseNotesByVersion(version: string, env: Env): Promise<Response> {
  const key = `release-notes/v${version}.json`
  const object = await env.RELEASES_BUCKET.get(key)
  if (!object) {
    return jsonResponse({ error: `Release notes not found for v${version}` }, 404)
  }
  const headers = r2HeadersToResponse(object, {
    'Content-Type': 'application/json',
    'Cache-Control': 'public, max-age=300',
  })
  return new Response(object.body, { status: 200, headers })
}

async function handleReleaseNotesIndex(channel: string, env: Env): Promise<Response> {
  if (!VALID_CHANNELS.has(channel)) {
    return jsonResponse({ error: `Invalid channel: ${channel}` }, 400)
  }
  const key = `release-notes/index/${channel}.json`
  const object = await env.RELEASES_BUCKET.get(key)
  if (!object) {
    return jsonResponse({ error: `Index not found for channel: ${channel}` }, 404)
  }
  const headers = r2HeadersToResponse(object, {
    'Content-Type': 'application/json',
    'Cache-Control': 'public, max-age=60',
  })
  return new Response(object.body, { status: 200, headers })
}

async function handleArtifact(version: string, filename: string, env: Env): Promise<Response> {
  const key = `artifacts/v${version}/${filename}`
  const object = await env.RELEASES_BUCKET.get(key)

  if (!object) {
    return jsonResponse({ error: 'Artifact not found' }, 404)
  }

  const contentType = inferContentType(filename)

  const headers = r2HeadersToResponse(object, {
    'Content-Type': contentType,
    'Cache-Control': 'public, max-age=86400, immutable',
    'Content-Disposition': `attachment; filename="${filename}"`,
  })

  return new Response(object.body, { status: 200, headers })
}

function inferContentType(filename: string): string {
  if (filename.endsWith('.tar.gz')) return 'application/gzip'
  if (filename.endsWith('.sig')) return 'application/octet-stream'
  if (filename.endsWith('.dmg')) return 'application/x-apple-diskimage'
  if (filename.endsWith('.deb')) return 'application/vnd.debian.binary-package'
  if (filename.endsWith('.AppImage')) return 'application/x-executable'
  if (filename.endsWith('.msi')) return 'application/x-msi'
  if (filename.endsWith('.exe')) return 'application/x-msdownload'
  if (filename.endsWith('.zip')) return 'application/zip'
  if (filename.endsWith('.json')) return 'application/json'
  return 'application/octet-stream'
}

function handleHealth(): Response {
  return jsonResponse({ status: 'ok', service: 'uniclipboard-update-server' }, 200)
}

// Allow either canonical semver (1.2.3, 1.2.3-alpha.4) or any safe path-segment
// shape; R2 keys can't path-traverse so the regex is for intent, not security.
const VERSION_PATH_REGEX = /^\/release-notes\/v([0-9A-Za-z.\-+]+)\.json$/

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: CORS_HEADERS })
    }

    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return jsonResponse({ error: 'Method not allowed' }, 405)
    }

    const url = new URL(request.url)
    const path = url.pathname

    // GET /health
    if (path === '/health') {
      return handleHealth()
    }

    // GET /release-notes/{channel}.json (channel index) — match strictly lowercase
    // before the v-prefixed version pattern so they don't overlap.
    const releaseNotesIndexMatch = path.match(/^\/release-notes\/([a-z]+)\.json$/)
    if (releaseNotesIndexMatch) {
      return handleReleaseNotesIndex(releaseNotesIndexMatch[1], env)
    }

    // GET /release-notes/v{version}.json
    const releaseNotesVersionMatch = path.match(VERSION_PATH_REGEX)
    if (releaseNotesVersionMatch) {
      return handleReleaseNotesByVersion(releaseNotesVersionMatch[1], env)
    }

    // GET /{channel}.json (with optional ?from=)
    // Only the `?from=` variant needs explicit Cache API handling, because the
    // default edge cache key excludes query strings and would otherwise alias
    // distinct `from` values to the same entry. Other routes use static URLs
    // and rely on Cloudflare's automatic Cache-Control handling.
    const channelMatch = path.match(/^\/([a-z]+)\.json$/)
    if (channelMatch) {
      const fromVersion = url.searchParams.get('from')
      return handleChannelManifest(request, channelMatch[1], fromVersion, env, ctx)
    }

    // GET /artifacts/v{version}/{filename}
    const artifactMatch = path.match(/^\/artifacts\/v([^/]+)\/(.+)$/)
    if (artifactMatch) {
      return handleArtifact(artifactMatch[1], artifactMatch[2], env)
    }

    return jsonResponse({ error: 'Not found' }, 404)
  },
}
