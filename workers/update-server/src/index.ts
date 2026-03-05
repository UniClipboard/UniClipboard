/**
 * UniClipboard Update Server
 *
 * Cloudflare Worker that serves update manifests and binary artifacts from R2.
 *
 * Routes:
 *   GET /{channel}.json          → Update manifest (60s cache)
 *   GET /artifacts/v{ver}/{file} → Binary download (24h cache, immutable)
 *   GET /health                  → Health check
 */

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

async function handleManifest(channel: string, env: Env): Promise<Response> {
  if (!VALID_CHANNELS.has(channel)) {
    return jsonResponse({ error: `Invalid channel: ${channel}` }, 400)
  }

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

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
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

    // GET /{channel}.json
    const channelMatch = path.match(/^\/([a-z]+)\.json$/)
    if (channelMatch) {
      return handleManifest(channelMatch[1], env)
    }

    // GET /artifacts/v{version}/{filename}
    const artifactMatch = path.match(/^\/artifacts\/v([^/]+)\/(.+)$/)
    if (artifactMatch) {
      return handleArtifact(artifactMatch[1], artifactMatch[2], env)
    }

    return jsonResponse({ error: 'Not found' }, 404)
  },
}
