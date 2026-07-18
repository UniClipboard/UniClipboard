import { describe, expect, it, vi } from 'vitest'
import worker from '../index'
import type { AndroidManifest } from '../types'

const context = {
  waitUntil: vi.fn(),
  passThroughOnException: vi.fn(),
} as unknown as ExecutionContext

function r2Object(body: string): R2ObjectBody {
  return {
    body: new Response(body).body,
    size: new TextEncoder().encode(body).byteLength,
    httpEtag: '"test-etag"',
  } as unknown as R2ObjectBody
}

function envWith(get: ReturnType<typeof vi.fn>): { RELEASES_BUCKET: R2Bucket } {
  return {
    RELEASES_BUCKET: { get } as unknown as R2Bucket,
  }
}

describe('Android update routes', () => {
  it('serves a channel manifest from its Android R2 key', async () => {
    const manifest: AndroidManifest = {
      version: '0.19.0',
      tagName: 'v0.19.0',
      prerelease: false,
      pub_date: '2026-06-01T00:00:00Z',
      notes: { en: 'Release notes', zh: 'Release notes zh' },
      assets: [{ name: 'uniclipboard-arm64-v8a.apk', sha256: 'abc123' }],
    }
    const get = vi.fn(async () => r2Object(JSON.stringify(manifest)))

    const response = await worker.fetch(
      new Request('https://updates.example/android/stable.json', {
        headers: { Origin: 'https://www.uniclipboard.app' },
      }),
      envWith(get),
      context
    )

    expect(get).toHaveBeenCalledWith('android/stable.json')
    expect(response.status).toBe(200)
    expect(response.headers.get('Content-Type')).toBe('application/json')
    expect(response.headers.get('Cache-Control')).toBe('public, max-age=60')
    expect(response.headers.get('Access-Control-Allow-Origin')).toBe('https://www.uniclipboard.app')
    expect(await response.json()).toEqual(manifest)
  })

  it('rejects unsupported Android channels before reading R2', async () => {
    const get = vi.fn()

    const response = await worker.fetch(
      new Request('https://updates.example/android/alpha.json'),
      envWith(get),
      context
    )

    expect(response.status).toBe(400)
    expect(get).not.toHaveBeenCalled()
    expect(await response.json()).toEqual({ error: 'Invalid android channel: alpha' })
  })

  it('serves APK artifacts with download headers', async () => {
    const get = vi.fn(async () => r2Object('apk bytes'))

    const response = await worker.fetch(
      new Request('https://updates.example/android/artifacts/v0.19.0/uniclipboard-arm64-v8a.apk'),
      envWith(get),
      context
    )

    expect(get).toHaveBeenCalledWith('android/artifacts/v0.19.0/uniclipboard-arm64-v8a.apk')
    expect(response.status).toBe(200)
    expect(response.headers.get('Content-Type')).toBe('application/vnd.android.package-archive')
    expect(response.headers.get('Cache-Control')).toBe('public, max-age=86400, immutable')
    expect(response.headers.get('Content-Disposition')).toBe(
      'attachment; filename="uniclipboard-arm64-v8a.apk"'
    )
    expect(await response.text()).toBe('apk bytes')
  })

  it('does not grant browser access to unknown origins', async () => {
    const response = await worker.fetch(
      new Request('https://updates.example/health', {
        headers: { Origin: 'https://attacker.example' },
      }),
      envWith(vi.fn()),
      context
    )

    expect(response.status).toBe(200)
    expect(response.headers.has('Access-Control-Allow-Origin')).toBe(false)
  })
})
