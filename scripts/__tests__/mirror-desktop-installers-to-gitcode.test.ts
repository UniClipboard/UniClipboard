import { execFile } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

const script = join(import.meta.dirname, '..', 'mirror-desktop-installers-to-gitcode.mjs')

const TOKEN = 'gitcode-secret-token-value'
const ACCESS_ID = 'access-client-id'
const ACCESS_SECRET = 'access-client-secret'
const TAG = 'v1.1.0'
const VERSION = '1.1.0'
const FILE_A = 'UniClipboard_1.1.0_aarch64.app.tar.gz'
const FILE_B = 'uniclipboard_1.1.0_amd64.AppImage.tar.gz'

type Behavior = {
  uploadFailures: number
  uploadAlwaysFails: boolean
  corruptDownload: boolean
  registrationStatus: number
}

type Fake = {
  port: number
  stored: Map<string, Buffer>
  requests: Array<{ method: string; url: string; headers: Record<string, string | undefined> }>
  registrations: Array<Record<string, unknown>>
  close: () => Promise<void>
}

function readBody(request: IncomingMessage): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = []
    request.on('data', (chunk: Buffer) => chunks.push(chunk))
    request.on('end', () => resolve(Buffer.concat(chunks)))
    request.on('error', reject)
  })
}

function json(response: ServerResponse, status: number, value: unknown) {
  response.writeHead(status, { 'content-type': 'application/json' })
  response.end(JSON.stringify(value))
}

async function startFake(overrides: Partial<Behavior> = {}): Promise<Fake> {
  const behavior: Behavior = {
    uploadFailures: 0,
    uploadAlwaysFails: false,
    corruptDownload: false,
    registrationStatus: 200,
    ...overrides,
  }
  const stored = new Map<string, Buffer>()
  const requests: Fake['requests'] = []
  const registrations: Array<Record<string, unknown>> = []
  let release: { tag_name: string } | null = null
  let uploadAttempts = 0
  let port = 0

  const base = () => `http://127.0.0.1:${port}`
  const assetList = () =>
    [...stored.keys()].map(name => ({
      name,
      browser_download_url: `${base()}/o/r/releases/download/${TAG}/${name}`,
      type: 'attach',
      id: `id-${name}`,
    }))

  const server: Server = createServer(async (request, response) => {
    const url = new URL(request.url ?? '/', base())
    requests.push({
      method: request.method ?? 'GET',
      url: `${url.pathname}${url.search}`,
      headers: {
        'cf-access-client-id': request.headers['cf-access-client-id'] as string | undefined,
        'cf-access-client-secret': request.headers['cf-access-client-secret'] as string | undefined,
      },
    })
    const path = url.pathname
    const apiRelease = `/api/v5/repos/o/r/releases`

    if (request.method === 'GET' && path === `${apiRelease}/tags/${TAG}`) {
      if (url.searchParams.get('access_token') !== TOKEN) return json(response, 401, {})
      if (!release) return json(response, 404, { error_message: 'Release not found' })
      return json(response, 200, { ...release, assets: assetList() })
    }
    if (request.method === 'POST' && path === apiRelease) {
      if (url.searchParams.get('access_token') !== TOKEN) return json(response, 401, {})
      const body = JSON.parse((await readBody(request)).toString('utf8'))
      release = { tag_name: body.tag_name }
      return json(response, 200, { ...release, ...body, assets: [] })
    }
    if (request.method === 'GET' && path === `${apiRelease}/${TAG}/upload_url`) {
      if (url.searchParams.get('access_token') !== TOKEN) return json(response, 401, {})
      const name = url.searchParams.get('file_name') ?? ''
      return json(response, 200, {
        url: `${base()}/obs/${encodeURIComponent(name)}`,
        headers: { 'x-obs-acl': 'public-read', 'content-type': 'application/octet-stream' },
      })
    }
    if (request.method === 'PUT' && path.startsWith('/obs/')) {
      const name = decodeURIComponent(path.slice('/obs/'.length))
      const body = await readBody(request)
      uploadAttempts += 1
      if (behavior.uploadAlwaysFails || uploadAttempts <= behavior.uploadFailures) {
        response.writeHead(500)
        return response.end('upload failed')
      }
      stored.set(name, body)
      response.writeHead(200)
      return response.end()
    }
    const direct =
      path.startsWith(`${apiRelease}/${TAG}/attach_files/`) && path.endsWith('/download')
        ? decodeURIComponent(
            path.slice(`${apiRelease}/${TAG}/attach_files/`.length, -'/download'.length)
          )
        : path.startsWith(`/o/r/releases/download/${TAG}/`)
          ? decodeURIComponent(path.slice(`/o/r/releases/download/${TAG}/`.length))
          : null
    if (request.method === 'GET' && direct !== null) {
      const bytes = stored.get(direct)
      if (!bytes) {
        response.writeHead(404)
        return response.end()
      }
      const body = behavior.corruptDownload
        ? Buffer.concat([bytes.subarray(0, -1), Buffer.from([0])])
        : bytes
      const range = request.headers.range
      if (range) {
        response.writeHead(206, {
          'content-range': `bytes 0-0/${body.length}`,
          'content-length': 1,
        })
        return response.end(body.subarray(0, 1))
      }
      response.writeHead(200, { 'content-length': body.length })
      return response.end(body)
    }
    if (request.method === 'PUT' && path === '/api/mirrors') {
      if (
        request.headers['cf-access-client-id'] !== ACCESS_ID ||
        request.headers['cf-access-client-secret'] !== ACCESS_SECRET
      ) {
        return json(response, 401, { error: 'Cloudflare Access identity required' })
      }
      const body = JSON.parse((await readBody(request)).toString('utf8'))
      if (behavior.registrationStatus !== 200) {
        return json(response, behavior.registrationStatus, {
          error: 'Mirror sha256 does not match the artifact',
        })
      }
      registrations.push(body)
      return json(response, 200, { data: { state: 'ready', ...body } })
    }
    response.writeHead(404)
    response.end()
  })

  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  port = (server.address() as AddressInfo).port
  return {
    get port() {
      return port
    },
    stored,
    requests,
    registrations,
    close: () =>
      new Promise<void>(resolve => {
        server.close(() => resolve())
        server.closeAllConnections()
      }),
  } as Fake
}

type Result = { code: number; stdout: string; stderr: string }

function run(args: string[], env: Record<string, string | undefined>): Promise<Result> {
  return new Promise(resolvePromise => {
    execFile(
      process.execPath,
      [script, ...args],
      {
        env: Object.fromEntries(
          Object.entries({ PATH: process.env.PATH, ...env }).filter(
            ([, value]) => value !== undefined
          )
        ) as NodeJS.ProcessEnv,
        timeout: 60_000,
        maxBuffer: 10 * 1024 * 1024,
      },
      (error, stdout, stderr) => {
        const code = error
          ? ((error as NodeJS.ErrnoException & { code?: number }).code as number)
          : 0
        resolvePromise({ code: typeof code === 'number' ? code : 1, stdout, stderr })
      }
    )
  })
}

describe('GitCode desktop installer mirror', () => {
  const dirs: string[] = []
  const servers: Fake[] = []
  const fileA = randomBytes(1024 + 13)
  const fileB = randomBytes(2048 + 7)
  const sha256A = createHash('sha256').update(fileA).digest('hex')
  const sha256B = createHash('sha256').update(fileB).digest('hex')

  afterEach(async () => {
    for (const server of servers.splice(0)) await server.close()
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true })
  })

  async function setup(overrides: Partial<Behavior> = {}) {
    const fake = await startFake(overrides)
    servers.push(fake)
    const dir = mkdtempSync(join(tmpdir(), 'gitcode-mirror-'))
    dirs.push(dir)
    const artifactsDir = join(dir, 'release-assets')
    writeFileSync(join(dir, 'placeholder'), '')
    const { mkdirSync } = await import('node:fs')
    mkdirSync(artifactsDir, { recursive: true })
    writeFileSync(join(artifactsDir, FILE_A), fileA)
    writeFileSync(join(artifactsDir, FILE_B), fileB)

    const registrationPath = join(dir, 'registration.json')
    writeFileSync(
      registrationPath,
      JSON.stringify({
        product: 'desktop',
        version: VERSION,
        tagName: TAG,
        prerelease: false,
        artifacts: [
          { platform: 'darwin-aarch64', filename: FILE_A },
          { platform: 'linux-x86_64', filename: FILE_B },
        ],
      })
    )

    const provenance = join(dir, 'provenance.json')
    const args = [
      '--registration',
      registrationPath,
      '--artifacts-dir',
      artifactsDir,
      '--source',
      'github-actions:UniClipboard/UniClipboard:1',
      '--provenance',
      provenance,
      '--allow-local-http',
      '--retry-delay-ms',
      '1',
      '--poll-interval-ms',
      '1',
    ]
    const env = {
      GITCODE_TOKEN: TOKEN,
      GITCODE_OWNER: 'o',
      GITCODE_REPO: 'r',
      GITCODE_API_BASE: `http://127.0.0.1:${fake.port}/api/v5`,
      FLARE_RELEASE_ADMIN_URL: `http://127.0.0.1:${fake.port}`,
      FLARE_RELEASE_ACCESS_CLIENT_ID: ACCESS_ID,
      FLARE_RELEASE_ACCESS_CLIENT_SECRET: ACCESS_SECRET,
    }
    return {
      fake,
      args,
      env,
      provenance: () =>
        JSON.parse(readFileSync(provenance, 'utf8')) as Array<Record<string, unknown>>,
    }
  }

  const putCount = (fake: Fake) =>
    fake.requests.filter(r => r.method === 'PUT' && r.url.startsWith('/obs/')).length

  it('mirrors every artifact, verifies anonymous downloads and registers both with FlareRelease', async () => {
    const { fake, args, env, provenance } = await setup()
    const result = await run(args, env)
    expect(result.code).toBe(0)
    expect(fake.stored.get(FILE_A)?.equals(fileA)).toBe(true)
    expect(fake.stored.get(FILE_B)?.equals(fileB)).toBe(true)
    expect(fake.registrations).toHaveLength(2)
    expect(fake.registrations).toContainEqual(
      expect.objectContaining({
        product: 'desktop',
        filename: FILE_A,
        size: fileA.length,
        sha256: sha256A,
      })
    )
    expect(fake.registrations).toContainEqual(
      expect.objectContaining({
        product: 'desktop',
        filename: FILE_B,
        size: fileB.length,
        sha256: sha256B,
      })
    )
    const records = provenance()
    expect(records).toHaveLength(2)
    expect(records.every(record => record.status === 'mirrored')).toBe(true)
    expect(JSON.stringify(records) + result.stdout + result.stderr).not.toContain(TOKEN)
    expect(JSON.stringify(records) + result.stdout + result.stderr).not.toContain(ACCESS_SECRET)
  })

  it('verifies downloads without any credential', async () => {
    const { fake, args, env } = await setup()
    await run(args, env)
    const downloads = fake.requests.filter(
      r => r.method === 'GET' && r.url.includes('/releases/download/')
    )
    expect(downloads.length).toBeGreaterThan(0)
    for (const download of downloads) {
      expect(download.url).not.toContain('access_token')
    }
  })

  it('is idempotent: a rerun reuses both uploads and registers again', async () => {
    const { fake, args, env } = await setup()
    expect((await run(args, env)).code).toBe(0)
    expect((await run(args, env)).code).toBe(0)
    expect(putCount(fake)).toBe(2)
    expect(fake.registrations).toHaveLength(4)
  })

  it('refuses to overwrite a differing file but still mirrors the other artifact', async () => {
    const { fake, args, env, provenance } = await setup()
    await run(args, env)
    fake.stored.set(FILE_A, randomBytes(64))
    fake.registrations.length = 0
    const result = await run(args, env)
    expect(result.code).toBe(1)
    expect(fake.requests.some(r => r.method === 'DELETE')).toBe(false)
    const records = provenance()
    const recordA = records.find(record => record.filename === FILE_A)
    const recordB = records.find(record => record.filename === FILE_B)
    expect(recordA).toMatchObject({ status: 'failed' })
    expect(recordB).toMatchObject({ status: 'mirrored' })
    expect(fake.registrations).toHaveLength(1)
  })

  it('retries a failed upload with a fresh upload address', async () => {
    const { fake, args, env } = await setup({ uploadFailures: 2 })
    const result = await run(args, env)
    expect(result.code).toBe(0)
    expect(fake.requests.filter(r => r.url.includes('/upload_url'))).toHaveLength(2 + 2)
  })

  it('does not register a mirror whose downloaded bytes differ from the artifact', async () => {
    const { fake, args, env, provenance } = await setup({ corruptDownload: true })
    const result = await run(args, env)
    expect(result.code).toBe(1)
    expect(fake.registrations).toHaveLength(0)
    const records = provenance()
    expect(records.every(record => /sha256|size/i.test(String(record.error)))).toBe(true)
  })

  it('skips with a warning when GitCode is not configured, or fails when asked to', async () => {
    const { fake, args, env, provenance } = await setup()
    const unconfigured = { ...env, GITCODE_TOKEN: undefined, GITCODE_OWNER: undefined }
    const skipped = await run([...args, '--missing-config', 'skip'], unconfigured)
    expect(skipped.code).toBe(0)
    expect(skipped.stdout + skipped.stderr).toMatch(/::warning/)
    expect(provenance().every(record => record.status === 'skipped')).toBe(true)
    expect(fake.requests).toHaveLength(0)

    const failed = await run([...args, '--missing-config', 'fail'], unconfigured)
    expect(failed.code).toBe(1)
    expect(fake.requests).toHaveLength(0)
  })

  it('only accepts https GitCode addresses outside local testing', async () => {
    const { fake, args, env } = await setup()
    const result = await run(
      args.filter(arg => arg !== '--allow-local-http'),
      env
    )
    expect(result.code).toBe(1)
    expect(fake.requests).toHaveLength(0)
  })

  it('reports an unreachable FlareRelease rejection without registering', async () => {
    const { provenance, args, env } = await setup({ registrationStatus: 422 })
    const result = await run(args, env)
    expect(result.code).toBe(1)
    const records = provenance()
    expect(records.every(record => record.status === 'failed')).toBe(true)
    expect(records.every(record => String(record.error).includes('422'))).toBe(true)
  })
})
