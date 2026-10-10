import { execFile } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer, type IncomingMessage, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

const script = join(import.meta.dirname, '..', 'register-flare-release-mirrors.mjs')

const ACCESS_ID = 'access-client-id'
const ACCESS_SECRET = 'access-client-secret'
const FILE_A = 'UniClipboard_aarch64-apple-darwin.app.tar.gz'
const FILE_B = 'UniClipboard_1.1.2_x64-setup.exe'
const SHA_A = 'a'.repeat(64)
const SHA_B = 'b'.repeat(64)
const URL_A = `https://gitcode.com/o/r/releases/download/v1.1.2/${FILE_A}`
const URL_B = `https://gitcode.com/o/r/releases/download/v1.1.2/${FILE_B}`

type Registration = Record<string, unknown>
type Fake = {
  port: number
  registrations: Registration[]
  attempts: Map<string, number>
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

// `statusFor` lets a test decide the response per filename and attempt number.
async function startFake(
  statusFor: (filename: string, attempt: number) => number = () => 200
): Promise<Fake> {
  const registrations: Registration[] = []
  const attempts = new Map<string, number>()
  const server: Server = createServer(async (request, response) => {
    if (request.method !== 'PUT' || request.url !== '/api/mirrors') {
      response.writeHead(404)
      return response.end()
    }
    if (
      request.headers['cf-access-client-id'] !== ACCESS_ID ||
      request.headers['cf-access-client-secret'] !== ACCESS_SECRET
    ) {
      response.writeHead(401, { 'content-type': 'application/json' })
      return response.end(JSON.stringify({ error: 'Cloudflare Access identity required' }))
    }
    const body = JSON.parse((await readBody(request)).toString('utf8')) as Registration
    const filename = String(body.filename)
    const attempt = (attempts.get(filename) ?? 0) + 1
    attempts.set(filename, attempt)
    const status = statusFor(filename, attempt)
    response.writeHead(status, { 'content-type': 'application/json' })
    if (status !== 200) {
      return response.end(JSON.stringify({ error: 'Mirror sha256 does not match the artifact' }))
    }
    registrations.push(body)
    response.end(JSON.stringify({ data: { state: 'ready', ...body } }))
  })
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
  const port = (server.address() as AddressInfo).port
  return {
    port,
    registrations,
    attempts,
    close: () => new Promise<void>(resolve => server.close(() => resolve())),
  }
}

type Result = { code: number; output: string }

function run(args: string[], env: Record<string, string | undefined>): Promise<Result> {
  return new Promise(resolve => {
    execFile(
      process.execPath,
      [script, ...args],
      { env: { PATH: process.env.PATH, ...env } },
      (error, stdout, stderr) => {
        const code = error && typeof error.code === 'number' ? error.code : error ? 1 : 0
        resolve({ code, output: `${stdout}${stderr}` })
      }
    )
  })
}

let cleanups: Array<() => Promise<void> | void> = []
afterEach(async () => {
  for (const cleanup of cleanups.reverse()) await cleanup()
  cleanups = []
})

function workdir(): string {
  const dir = mkdtempSync(join(tmpdir(), 'register-mirrors-'))
  cleanups.push(() => rmSync(dir, { recursive: true, force: true }))
  return dir
}

type ReceiptFile = {
  name: string
  status: string
  size?: number
  sha256?: string
  downloadUrl?: string
  stage?: string
  error?: string
}

function setup(
  receiptFiles: ReceiptFile[],
  artifacts: Array<{ filename: string; sha256: string }> = [
    { filename: FILE_A, sha256: SHA_A },
    { filename: FILE_B, sha256: SHA_B },
  ]
) {
  const dir = workdir()
  const receipt = join(dir, 'receipt.json')
  const registration = join(dir, 'registration.json')
  const provenance = join(dir, 'provenance.json')
  writeFileSync(receipt, JSON.stringify({ schema: 1, files: receiptFiles }))
  writeFileSync(
    registration,
    JSON.stringify({ product: 'desktop', version: '1.1.2', tagName: 'v1.1.2', artifacts })
  )
  return { receipt, registration, provenance }
}

const goodA: ReceiptFile = {
  name: FILE_A,
  status: 'mirrored',
  size: 10,
  sha256: SHA_A,
  downloadUrl: URL_A,
}
const goodB: ReceiptFile = {
  name: FILE_B,
  status: 'reused',
  size: 20,
  sha256: SHA_B,
  downloadUrl: URL_B,
}

async function register(
  paths: ReturnType<typeof setup>,
  fake: Fake,
  extra: string[] = [],
  env: Record<string, string | undefined> = {}
) {
  return run(
    [
      '--receipt',
      paths.receipt,
      '--registration',
      paths.registration,
      '--provenance',
      paths.provenance,
      '--source',
      'github-actions:o/r:1',
      '--allow-local-http',
      '--retry-delay-ms',
      '10',
      ...extra,
    ],
    {
      FLARE_RELEASE_ADMIN_URL: `http://127.0.0.1:${fake.port}`,
      FLARE_RELEASE_ACCESS_CLIENT_ID: ACCESS_ID,
      FLARE_RELEASE_ACCESS_CLIENT_SECRET: ACCESS_SECRET,
      ...env,
    }
  )
}

describe('register-flare-release-mirrors', () => {
  it('registers every mirrored or reused file with the receipt digest and GitCode address', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await register(paths, fake)
    expect(result.code).toBe(0)
    expect(fake.registrations).toEqual([
      {
        product: 'desktop',
        version: '1.1.2',
        filename: FILE_A,
        provider: 'gitcode',
        downloadUrl: URL_A,
        size: 10,
        sha256: SHA_A,
        source: 'github-actions:o/r:1',
      },
      {
        product: 'desktop',
        version: '1.1.2',
        filename: FILE_B,
        provider: 'gitcode',
        downloadUrl: URL_B,
        size: 20,
        sha256: SHA_B,
        source: 'github-actions:o/r:1',
      },
    ])
    const provenance = JSON.parse(readFileSync(paths.provenance, 'utf8'))
    expect(provenance.map((r: { status: string }) => r.status)).toEqual([
      'registered',
      'registered',
    ])
  })

  it('never registers a file the Action failed to mirror, and still registers the others', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([
      goodA,
      { name: FILE_B, status: 'failed', stage: 'upload', error: 'timed out' },
    ])
    const result = await register(paths, fake)
    expect(result.code).toBe(1)
    expect(fake.registrations.map(r => r.filename)).toEqual([FILE_A])
    expect(result.output).toContain(FILE_B)
  })

  it('refuses a receipt whose digest differs from the registered artifact', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([{ ...goodA, sha256: 'c'.repeat(64) }, goodB])
    const result = await register(paths, fake)
    expect(result.code).toBe(1)
    expect(fake.registrations.map(r => r.filename)).toEqual([FILE_B])
  })

  it('treats a registered artifact that is missing from the receipt as a failure', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([goodA])
    const result = await register(paths, fake)
    expect(result.code).toBe(1)
    expect(fake.registrations.map(r => r.filename)).toEqual([FILE_A])
  })

  it('refuses a download address outside GitCode or AtomGit', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([
      { ...goodA, downloadUrl: `https://evil.example.com/${FILE_A}` },
      { ...goodB, downloadUrl: `${URL_B}?token=1` },
    ])
    const result = await register(paths, fake)
    expect(result.code).toBe(1)
    expect(fake.registrations).toEqual([])
  })

  it('retries a transient FlareRelease failure and then succeeds', async () => {
    const fake = await startFake((_filename, attempt) => (attempt < 3 ? 503 : 200))
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await register(paths, fake)
    expect(result.code).toBe(0)
    expect(fake.attempts.get(FILE_A)).toBe(3)
    expect(fake.registrations).toHaveLength(2)
  })

  it('does not retry a rejected registration (422) and reports it', async () => {
    const fake = await startFake(filename => (filename === FILE_A ? 422 : 200))
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await register(paths, fake)
    expect(result.code).toBe(1)
    expect(fake.attempts.get(FILE_A)).toBe(1)
    expect(fake.registrations.map(r => r.filename)).toEqual([FILE_B])
    expect(result.output).toContain('422')
  })

  it('fails clearly when FlareRelease credentials are missing, without contacting it', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await register(paths, fake, [], {
      FLARE_RELEASE_ACCESS_CLIENT_ID: '',
      FLARE_RELEASE_ACCESS_CLIENT_SECRET: '',
    })
    expect(result.code).toBe(1)
    expect(result.output).toContain('FLARE_RELEASE_ACCESS_CLIENT_ID')
    expect(fake.attempts.size).toBe(0)
  })

  it('fails on an unreadable or empty receipt', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([])
    const empty = await register(paths, fake)
    expect(empty.code).toBe(1)
    writeFileSync(paths.receipt, 'not json')
    const broken = await register(paths, fake)
    expect(broken.code).toBe(1)
    expect(fake.attempts.size).toBe(0)
  })

  it('does not print FlareRelease credentials', async () => {
    const fake = await startFake(() => 401)
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await register(paths, fake, [], {
      FLARE_RELEASE_ACCESS_CLIENT_ID: 'wrong-id',
      FLARE_RELEASE_ACCESS_CLIENT_SECRET: ACCESS_SECRET,
    })
    expect(result.code).toBe(1)
    expect(result.output).not.toContain(ACCESS_SECRET)
    expect(readFileSync(paths.provenance, 'utf8')).not.toContain(ACCESS_SECRET)
  })

  it('requires an https admin address unless local testing is allowed', async () => {
    const fake = await startFake()
    cleanups.push(fake.close)
    const paths = setup([goodA, goodB])
    const result = await run(['--receipt', paths.receipt, '--registration', paths.registration], {
      FLARE_RELEASE_ADMIN_URL: `http://127.0.0.1:${fake.port}`,
      FLARE_RELEASE_ACCESS_CLIENT_ID: ACCESS_ID,
      FLARE_RELEASE_ACCESS_CLIENT_SECRET: ACCESS_SECRET,
    })
    expect(result.code).toBe(1)
    expect(fake.attempts.size).toBe(0)
  })
})
