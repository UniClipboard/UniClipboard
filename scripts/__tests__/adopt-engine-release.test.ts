import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'

const repositoryRoot = resolve(__dirname, '../..')
const adopter = resolve(repositoryRoot, 'scripts/adopt-engine-release.mjs')
const preflight = resolve(repositoryRoot, 'scripts/architecture/check-engine-repository.mjs')
const workflowPath = resolve(repositoryRoot, '.github/workflows/adopt-engine-release.yml')
const interopScript = resolve(repositoryRoot, 'scripts/test_pair_e2e.sh')
const roots: string[] = []

function write(path: string, content: string) {
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, content)
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'desktop-engine-adoption-'))
  roots.push(root)
  const oldCommit = 'a'.repeat(40)
  const newCommit = 'b'.repeat(40)
  const version = 'v1.2.3-rc.4'
  const sourceArchive = Buffer.from('verified source archive')
  const manifest = {
    schemaVersion: 1,
    release: { version, commit: newCommit },
    artifacts: [
      {
        name: 'UniClipboardEngine-source.tar.gz',
        sha256: createHash('sha256').update(sourceArchive).digest('hex'),
        size: sourceArchive.length,
      },
    ],
  }
  const manifestPath = join(root, 'release-manifest.json')
  const archivePath = join(root, 'UniClipboardEngine-source.tar.gz')
  write(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  write(archivePath, sourceArchive.toString())
  write(
    join(root, 'Cargo.toml'),
    `[workspace.dependencies]\nuc-engine = { git = "https://github.com/UniClipboard/Engine.git", rev = "${oldCommit}" }\nuc-observability-contract = { git = "https://github.com/UniClipboard/Engine.git", rev = "${oldCommit}" }\n`
  )
  return { root, version, newCommit, manifestPath, archivePath }
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('desktop Engine release adoption', () => {
  it('updates every desktop Engine dependency after verifying the release source', () => {
    const { root, version, newCommit, manifestPath, archivePath } = fixture()

    execFileSync(
      process.execPath,
      [
        adopter,
        '--root',
        root,
        '--version',
        version,
        '--source-commit',
        newCommit,
        '--manifest',
        manifestPath,
        '--source-archive',
        archivePath,
      ],
      { encoding: 'utf8' }
    )

    const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
    expect(cargo.match(new RegExp(newCommit, 'g'))).toHaveLength(2)
    expect(cargo).not.toContain('a'.repeat(40))
  })

  it('rejects a notification that disagrees with the public release manifest', () => {
    const { root, version, manifestPath, archivePath } = fixture()
    const result = spawnSync(
      process.execPath,
      [
        adopter,
        '--root',
        root,
        '--version',
        version,
        '--source-commit',
        'c'.repeat(40),
        '--manifest',
        manifestPath,
        '--source-archive',
        archivePath,
      ],
      { encoding: 'utf8' }
    )

    expect(result.status).toBe(1)
    expect(result.stderr).toContain('release source commit does not match the notification')
  })

  it('derives the preflight revision instead of keeping a second hard-coded pin', () => {
    const source = readFileSync(preflight, 'utf8')
    expect(source).not.toMatch(/const ENGINE_REVISION = ['"][0-9a-f]{40}['"]/)
    expect(source).toContain('resolveEnginePin')
  })

  it('checks the update before creating one deterministic pull request', () => {
    expect(existsSync(workflowPath)).toBe(true)
    const workflow = existsSync(workflowPath) ? readFileSync(workflowPath, 'utf8') : ''
    expect(workflow).toContain('types: [engine_release_published]')
    expect(workflow).toMatch(/permissions:\s*\n\s+contents: read\s*\n\s+packages: read/)
    expect(workflow).toContain('automation/adopt-engine-${{ github.event.client_payload.version }}')
    expect(workflow).toMatch(/create-pull-request:[\s\S]*needs:\s*validate/)
    expect(workflow).toContain('cargo check --workspace --locked')
    expect(workflow).toContain('bun run check:engine-repository')
    expect(workflow).toContain(
      'run: bun scripts/adopt-engine-release.mjs --version "$ENGINE_VERSION" --source-commit "$ENGINE_COMMIT"'
    )
    expect(workflow).not.toContain('run: node scripts/adopt-engine-release.mjs')
    expect(workflow).toContain('changed: ${{ steps.change.outputs.changed }}')
    expect(workflow).toContain("if: ${{ needs.validate.outputs.changed == 'true' }}")
    expect(workflow).toContain('--state all')
    expect(workflow).toContain('gh pr reopen')
    expect(workflow).toMatch(
      /- uses: actions\/checkout@v4\s+with:\s+ref: \$\{\{ needs\.validate\.outputs\.base_sha \}\}\s+fetch-depth: 0\s+token: \$\{\{ secrets\.REPO_BOT_TOKEN \}\}\s+persist-credentials: false/
    )
    expect(workflow).toMatch(
      /- name: Push the deterministic branch\s+env:\s+GH_TOKEN: \$\{\{ secrets\.REPO_BOT_TOKEN \}\}/
    )
    expect(workflow).toMatch(
      /- name: Create or update the pull request\s+env:\s+GH_TOKEN: \$\{\{ secrets\.REPO_BOT_TOKEN \}\}/
    )
    expect(workflow).toContain("git -c credential.helper= -c credential.helper='!f()")
    expect(workflow).not.toContain('actions/create-github-app-token')
    expect(workflow).not.toContain('ENGINE_RELEASE_APP_')
    expect(workflow).not.toContain('GH_TOKEN: ${{ github.token }}')
  })

  it('lets the interop gate run old and new clients and verify both transfer directions', () => {
    const script = readFileSync(interopScript, 'utf8')
    expect(script).toContain('ALICE_CLI')
    expect(script).toContain('BOB_CLI')
    expect(script).toContain('verify_transfer "$ALICE_CLI" "$BOB_CLI"')
    expect(script).toContain('verify_transfer "$BOB_CLI" "$ALICE_CLI"')
  })

  it('stops the pending invitation immediately when the joiner fails', () => {
    const root = mkdtempSync(join(tmpdir(), 'desktop-engine-interop-failure-'))
    roots.push(root)
    const fakeCli = join(root, 'fake-uniclip')
    write(
      fakeCli,
      `#!/usr/bin/env bash
if [[ " $* " == *" invite "* ]]; then
  echo "INVITATION_CODE=1234-5678"
  sleep 4
  exit 0
fi
if [[ " $* " == *" join "* ]]; then
  exit 23
fi
exit 0
`
    )
    chmodSync(fakeCli, 0o755)

    const startedAt = Date.now()
    const result = spawnSync('bash', [interopScript], {
      cwd: repositoryRoot,
      encoding: 'utf8',
      env: {
        ...process.env,
        HOME: root,
        TMPDIR: root,
        ALICE_CLI: fakeCli,
        BOB_CLI: fakeCli,
        PROFILE_PREFIX: 'join-failure-test',
        WAIT_SECS: '1',
      },
    })

    expect(result.status).toBe(1)
    expect(Date.now() - startedAt).toBeLessThan(2500)
    expect(result.stderr).toContain('FAIL: bob exit status 23')
  })
})
