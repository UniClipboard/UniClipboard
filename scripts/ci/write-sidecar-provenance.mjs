#!/usr/bin/env node
// Record where the staged sidecar binaries came from, next to them in target/sidecar-staging/.
//
// Packagers verify the daemon they ship against this file: the SHA-256 must match the file they
// were given, and the recorded source commit must be the commit they package from. The file is
// written by the same job that built the binaries, so it is build evidence, not a claim made by
// the packaging job.
//
// Usage: node scripts/ci/write-sidecar-provenance.mjs --target <triple> --build-mode <release|test>
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..')
const stagingDir = join(repoRoot, 'target', 'sidecar-staging')

function arg(name) {
  const i = process.argv.indexOf(name)
  if (i < 0 || !process.argv[i + 1]) throw new Error(`${name} is required`)
  return process.argv[i + 1]
}

const sha256 = path => createHash('sha256').update(readFileSync(path)).digest('hex')
const git = (...args) => execFileSync('git', args, { cwd: repoRoot, encoding: 'utf8' }).trim()

const target = arg('--target')
const buildMode = arg('--build-mode')
const files = {}
for (const name of readdirSync(stagingDir).sort()) {
  if (name === 'sidecar-provenance.json') continue
  files[name] = {
    sha256: sha256(join(stagingDir, name)),
    bytes: statSync(join(stagingDir, name)).size,
  }
}
if (Object.keys(files).length === 0) throw new Error(`${stagingDir} holds no staged binaries`)

const env = process.env
const provenance = {
  schema: 1,
  target,
  buildMode,
  // The commit that was actually checked out and built, not github.sha: the build may check out
  // inputs.branch.
  sourceHead: git('rev-parse', 'HEAD'),
  sourceDirty: git('status', '--porcelain') !== '',
  rustc: execFileSync('rustc', ['-Vv'], { cwd: repoRoot, encoding: 'utf8' }).trim(),
  cargoLockSha256: sha256(join(repoRoot, 'Cargo.lock')),
  run: env.GITHUB_RUN_ID
    ? {
        repository: env.GITHUB_REPOSITORY,
        id: env.GITHUB_RUN_ID,
        attempt: env.GITHUB_RUN_ATTEMPT,
        workflow: env.GITHUB_WORKFLOW,
        url: `${env.GITHUB_SERVER_URL}/${env.GITHUB_REPOSITORY}/actions/runs/${env.GITHUB_RUN_ID}`,
      }
    : null,
  files,
}
writeFileSync(
  join(stagingDir, 'sidecar-provenance.json'),
  JSON.stringify(provenance, null, 2) + '\n'
)
console.log(`[sidecar] provenance for ${Object.keys(files).join(', ')} at ${provenance.sourceHead}`)
