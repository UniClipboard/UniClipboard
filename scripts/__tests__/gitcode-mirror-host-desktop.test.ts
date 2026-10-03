import { spawn } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { createServer, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

const root = join(import.meta.dirname, '..', '..')
const wrapper = join(root, 'scripts', 'remote', 'gitcode-mirror-host-desktop.py')

const TAG = 'v1.1.0-alpha.4'
const VERSION = '1.1.0-alpha.4'
const FILE_A = 'UniClipboard_1.1.0-alpha.4_aarch64.app.tar.gz'
const FILE_B = 'uniclipboard_1.1.0-alpha.4_amd64.AppImage.tar.gz'

// Stands in for the installed mirror-desktop-installers-to-gitcode.mjs: it
// reports what it was given so the tests can check the contract between CI
// and the mirror host, without any real network access.
const stubScript = `
import fs from 'node:fs'
import { readFileSync } from 'node:fs'
const args = process.argv.slice(2)
const registration = JSON.parse(readFileSync(args[args.indexOf('--registration') + 1], 'utf8'))
const artifactsDir = args[args.indexOf('--artifacts-dir') + 1]
console.log('STUB ' + JSON.stringify({
  args,
  registration,
  artifactBytes: registration.artifacts.map(a => fs.statSync(artifactsDir + '/' + a.filename).size),
  env: Object.fromEntries(Object.entries(process.env).filter(([k]) => /^(GITCODE|FLARE_RELEASE)_/.test(k) || k === 'LD_PRELOAD')),
}))
const out = args[args.indexOf('--provenance') + 1]
fs.writeFileSync(out, JSON.stringify({ status: 'mirrored', tag: args[args.indexOf('--tag') + 1] }))
process.exit(Number(process.env.STUB_EXIT || 0))
`

describe('Desktop GitCode mirror host wrapper', () => {
  const fileA = randomBytes(300 * 1024 + 7)
  const fileB = randomBytes(150 * 1024 + 3)
  const shaA = createHash('sha256').update(fileA).digest('hex')
  const shaB = createHash('sha256').update(fileB).digest('hex')
  let server: Server
  let base: string
  const downloads: string[] = []
  let work: string

  beforeEach(async () => {
    work = mkdtempSync(join(tmpdir(), 'mirror-host-desktop-'))
    downloads.length = 0
    server = createServer((request, response) => {
      downloads.push(request.url ?? '')
      if (request.url === `/artifacts/${TAG}/${FILE_A}`) {
        response.writeHead(200, { 'content-length': fileA.length })
        return response.end(fileA)
      }
      if (request.url === `/artifacts/${TAG}/${FILE_B}`) {
        response.writeHead(200, { 'content-length': fileB.length })
        return response.end(fileB)
      }
      response.writeHead(404)
      response.end()
    })
    await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve))
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
  })

  afterEach(async () => {
    await new Promise<void>(resolve => {
      server.close(() => resolve())
      server.closeAllConnections()
    })
    rmSync(work, { recursive: true, force: true })
  })

  type Request = {
    tag?: string
    version?: string
    scriptSha256?: string
    prerelease?: boolean
    source?: string
    artifacts?: Array<{ filename?: string; sha256?: string }>
    env?: Record<string, string>
  }

  function run(
    request: Request,
    options: { script?: string; trailing?: string; omitScriptFile?: boolean } = {}
  ) {
    const script = options.script ?? stubScript
    const installed = join(mkdtempSync(join(tmpdir(), 'mirror-installed-')), 'mirror.mjs')
    if (!options.omitScriptFile) writeFileSync(installed, script)
    return new Promise<{ code: number; stdout: string; stderr: string }>(resolve => {
      const child = spawn('python3', [wrapper], {
        env: {
          PATH: process.env.PATH,
          UNICLIP_MIRROR_R2_BASE: base,
          UNICLIP_MIRROR_NODE: process.execPath,
          UNICLIP_MIRROR_TMP: work,
          UNICLIP_MIRROR_SCRIPT: installed,
        },
      })
      let stdout = ''
      let stderr = ''
      child.stdout.on('data', chunk => (stdout += chunk))
      child.stderr.on('data', chunk => (stderr += chunk))
      child.on('close', code => {
        rmSync(join(installed, '..'), { recursive: true, force: true })
        resolve({ code: code ?? 1, stdout, stderr })
      })
      const header = {
        tag: TAG,
        version: VERSION,
        scriptSha256: createHash('sha256').update(script).digest('hex'),
        prerelease: false,
        source: 'github-actions:UniClipboard/UniClipboard:1',
        artifacts: [
          { filename: FILE_A, sha256: shaA },
          { filename: FILE_B, sha256: shaB },
        ],
        env: { GITCODE_TOKEN: 'tok-secret', GITCODE_OWNER: 'o', GITCODE_REPO: 'r' },
        ...request,
      }
      child.stdin.end(`${JSON.stringify(header)}\n${options.trailing ?? ''}`)
    })
  }

  const stubReport = (stdout: string) => JSON.parse(/STUB (.*)/.exec(stdout)?.[1] ?? 'null')

  it('downloads every artifact from R2, verifies it, runs the script once and hands back the provenance', async () => {
    const result = await run({})
    expect(result.code).toBe(0)
    const report = stubReport(result.stdout)
    expect(report.artifactBytes).toEqual([fileA.length, fileB.length])
    expect(report.registration).toMatchObject({
      version: VERSION,
      tagName: TAG,
      prerelease: false,
      artifacts: [{ filename: FILE_A }, { filename: FILE_B }],
    })
    expect(report.args).toEqual(
      expect.arrayContaining([
        '--tag',
        TAG,
        '--source',
        'github-actions:UniClipboard/UniClipboard:1',
      ])
    )
    expect(report.env).toMatchObject({
      GITCODE_TOKEN: 'tok-secret',
      GITCODE_OWNER: 'o',
      GITCODE_REPO: 'r',
    })
    expect(result.stdout).toContain('::provenance::')
    expect(result.stdout).toMatch(/::provenance::\{"status": ?"mirrored"/)
    expect(downloads.sort()).toEqual(
      [`/artifacts/${TAG}/${FILE_A}`, `/artifacts/${TAG}/${FILE_B}`].sort()
    )
  })

  it('passes the prerelease flag and removes everything it created', async () => {
    const result = await run({ prerelease: true })
    expect(stubReport(result.stdout).registration.prerelease).toBe(true)
    expect(readdirSync(work)).toEqual([])
  })

  it('refuses a download whose SHA-256 is not the expected one and never runs the script', async () => {
    const result = await run({
      artifacts: [
        { filename: FILE_A, sha256: '0'.repeat(64) },
        { filename: FILE_B, sha256: shaB },
      ],
    })
    expect(result.code).not.toBe(0)
    expect(result.stdout).not.toContain('STUB')
    expect(result.stdout + result.stderr).toMatch(/sha256/i)
    expect(readdirSync(work)).toEqual([])
  })

  it('fails when a file cannot be downloaded', async () => {
    const result = await run({ tag: 'v9.9.9' })
    expect(result.code).not.toBe(0)
    expect(result.stdout).not.toContain('STUB')
  })

  it('rejects anything that is not a plain tag, version, known installer name or digest', async () => {
    for (const bad of [
      { tag: '../etc' },
      { tag: 'v1; rm -rf /' },
      { version: '../x' },
      { artifacts: [{ filename: '../../x.exe', sha256: shaA }] },
      { artifacts: [{ filename: 'UniClipboard_1.1.0_x64.sh', sha256: shaA }] },
      { artifacts: [{ filename: FILE_A, sha256: 'abc' }] },
      { artifacts: [] },
      { artifacts: Array.from({ length: 9 }, () => ({ filename: FILE_A, sha256: shaA })) },
      {
        artifacts: [
          { filename: FILE_A, sha256: shaA },
          { filename: FILE_A, sha256: shaA },
        ],
      },
    ]) {
      const result = await run(bad)
      expect(result.code).not.toBe(0)
      expect(result.stdout).not.toContain('STUB')
    }
    expect(downloads).toEqual([])
  })

  it('only forwards the settings of the upload script, never other environment variables', async () => {
    const result = await run({ env: { GITCODE_TOKEN: 't', LD_PRELOAD: '/tmp/evil.so' } })
    expect(result.code).not.toBe(0)
    expect(result.stdout).not.toContain('STUB')
    expect(result.stdout + result.stderr).toMatch(/LD_PRELOAD/)
  })

  it('returns the exit status of the upload script and never echoes the secrets', async () => {
    const result = await run(
      {},
      {
        script: stubScript.replace(
          'process.exit(Number(process.env.STUB_EXIT || 0))',
          'process.exit(3)'
        ),
      }
    )
    expect(result.code).toBe(3)
    expect((result.stdout + result.stderr).replace(/STUB .*/, '')).not.toContain('tok-secret')
  })

  it('never runs code that arrives over the SSH session', async () => {
    const result = await run({}, { trailing: "console.log('STUB injected');" })
    expect(result.code).not.toBe(0)
    expect(result.stdout).not.toContain('STUB')
    expect(result.stdout + result.stderr).toMatch(/unexpected data|only the request/i)
    expect(downloads).toEqual([])
  })

  it('refuses to run an installed script other than the version CI expects', async () => {
    const result = await run({ scriptSha256: '1'.repeat(64) })
    expect(result.code).not.toBe(0)
    expect(result.stdout).not.toContain('STUB')
    expect(result.stdout + result.stderr).toMatch(/out of date|scriptSha256/i)
    expect(downloads).toEqual([])
  })

  it('fails clearly when the script is not installed on the host', async () => {
    const result = await run({}, { omitScriptFile: true })
    expect(result.code).not.toBe(0)
    expect(result.stdout + result.stderr).toMatch(/not installed|no such file/i)
    expect(downloads).toEqual([])
  })
})
