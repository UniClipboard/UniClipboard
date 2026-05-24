/**
 * UniClipboard 更新服务
 *
 * 基于 Cloudflare Worker，从 R2 派发更新 manifest 与二进制产物。
 *
 * 路由：
 *   GET /{channel}.json                          → 更新 manifest（缓存 60s）
 *   GET /{channel}.json?from={version}           → manifest（notes 字段已合并 (from, latest] 之间的版本，最多 5 个）
 *   GET /release-notes/v{version}.json           → 单版本归档发布日志
 *   GET /release-notes/{channel}.json            → channel 版本索引
 *   GET /artifacts/v{ver}/{file}                 → 二进制下载（缓存 24h，immutable）
 *   GET /health                                  → 健康检查
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
    // 把 JSON 损坏作为 server error 抛出，不要伪装成「对象不存在」。
    // 调用方约定 `null` 仅表示「R2 对象缺失」；在这里抛错保持两种失败语义清晰，
    // 排查时方向不会被误导。
    throw new Error(
      `Corrupted JSON at R2 key ${key}: ${err instanceof Error ? err.message : String(err)}`,
      { cause: err }
    )
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

  // 没有 `?from=`：直接透传 R2 对象。URL 无歧义时 Cloudflare 边缘缓存会按
  // Cache-Control 自动处理，无需手动 cache.put。
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

  // 带 `?from=`：动态拼接 notes 后返回 manifest。
  // Cloudflare 默认 cache key 不包含 query string，不同 `from` 值会撞到同一个缓存项；
  // 这里显式用完整 URL 作为 cache key 管理缓存。
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
  // 索引缺失（如首次部署、尚未回填）时降级为单版本 notes，不阻断升级流程。
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

// 允许标准 semver（如 1.2.3、1.2.3-alpha.4）以及任意安全的路径段字符；
// R2 key 本身无法 path-traverse，所以这个正则只是「表达意图」而非安全屏障。
const VERSION_PATH_REGEX = /^\/release-notes\/v([0-9A-Za-z.\-+]+)\.json$/

async function route(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
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

  // GET /release-notes/{channel}.json（channel 索引）—— 严格匹配纯小写字母，
  // 必须放在 v 前缀的版本路由之前，避免两者匹配重叠。
  const releaseNotesIndexMatch = path.match(/^\/release-notes\/([a-z]+)\.json$/)
  if (releaseNotesIndexMatch) {
    return handleReleaseNotesIndex(releaseNotesIndexMatch[1], env)
  }

  // GET /release-notes/v{version}.json
  const releaseNotesVersionMatch = path.match(VERSION_PATH_REGEX)
  if (releaseNotesVersionMatch) {
    return handleReleaseNotesByVersion(releaseNotesVersionMatch[1], env)
  }

  // GET /{channel}.json（可选 ?from=）
  // 只有带 `?from=` 的变体需要显式调用 Cache API：默认边缘 cache key 不含
  // query string，否则不同 `from` 值会被错认为同一个缓存项。其他路由 URL
  // 都是静态形态，依赖 Cloudflare 自动 Cache-Control 处理即可。
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
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    try {
      return await route(request, env, ctx)
    } catch (err) {
      // 兜底捕获意外错误（例如 getJsonFromR2 抛出的 R2 JSON 损坏），
      // 确保响应仍然带 CORS headers 与结构化 500 body，而不是 runtime 的裸错误页。
      console.error('Unhandled error:', err)
      return jsonResponse(
        {
          error: 'Internal server error',
          detail: err instanceof Error ? err.message : String(err),
        },
        500
      )
    }
  },
}
