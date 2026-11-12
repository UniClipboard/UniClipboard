// Run with Bun; all Git writes stay in an isolated local repository.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const root = path.resolve(import.meta.dirname, '../..')
const prepare = Bun.YAML.parse(
  fs.readFileSync(path.join(root, '.github/workflows/prepare-release.yml'), 'utf8')
)
const tag = Bun.YAML.parse(
  fs.readFileSync(path.join(root, '.github/workflows/tag-on-merge.yml'), 'utf8')
)
const artifact = path.resolve(
  process.argv[2] || path.join(os.tmpdir(), 'prepare-release-branches.json')
)
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'release-branches-'))
const cases = []
const git = (cwd, ...args) =>
  execFileSync('git', args, { cwd, encoding: 'utf8', stdio: 'pipe' }).trim()
const resolveBase = (expression, input) => {
  const match = expression.match(/^\$\{\{ inputs\.base_branch \|\| '([^']+)' \}\}$/)
  assert.ok(match, `Unexpected base expression: ${expression}`)
  return input || match[1]
}
const report = {
  source_sha: git(root, 'rev-parse', 'HEAD'),
  command: `bun scripts/ci/prepare-release-branches.e2e.mjs ${artifact}`,
  cases,
}
report.sha256 = Object.fromEntries(
  [
    '.github/workflows/prepare-release.yml',
    '.github/workflows/tag-on-merge.yml',
    'scripts/ci/prepare-release-branches.e2e.mjs',
  ].map(file => [
    file,
    createHash('sha256')
      .update(fs.readFileSync(path.join(root, file)))
      .digest('hex'),
  ])
)

try {
  const origin = path.join(temp, 'origin.git')
  const source = path.join(temp, 'source')
  git(temp, 'init', '--bare', origin)
  git(temp, 'init', '-b', 'v1', source)
  git(source, 'config', 'user.name', 'Release E2E')
  git(source, 'config', 'user.email', 'release@example.test')
  fs.writeFileSync(path.join(source, 'package.json'), JSON.stringify({ version: '1.6.2-alpha.2' }))
  git(source, 'add', '.')
  git(source, 'commit', '-m', 'Release fixture')
  git(source, 'remote', 'add', 'origin', origin)
  git(source, 'push', 'origin', 'v1', 'HEAD:release/v1.6.2-alpha.1')
  git(origin, 'symbolic-ref', 'HEAD', 'refs/heads/v1')
  const checkout = prepare.jobs.prepare.steps.find(step => step.name === 'Checkout repository')

  for (const input of ['', 'release/v1.6.2-alpha.1']) {
    const name = input || 'default'
    const base = resolveBase(
      checkout.with.ref,
      input || prepare.on.workflow_dispatch.inputs.base_branch.default
    )
    const repo = path.join(temp, name === 'default' ? name : 'explicit')
    git(temp, 'clone', '--branch', base, origin, repo)
    assert.equal(base, input || 'v1')
    assert.equal(resolveBase(prepare.env.BASE_BRANCH, input), base)
    assert.equal(
      resolveBase(prepare.concurrency.group.replace('prepare-release-', ''), input),
      base
    )
    assert.ok(
      tag.on.pull_request.branches.some(
        pattern => pattern === base || (pattern === 'release/v*' && base.startsWith('release/v'))
      )
    )
    git(repo, 'config', 'user.name', 'Release E2E')
    git(repo, 'config', 'user.email', 'release@example.test')
    const version = '1.6.2-alpha.2'
    const output = path.join(temp, `${name === 'default' ? name : 'explicit'}.output`)
    for (const stepName of [
      'Extract version from branch name',
      'Validate version matches package.json',
      'Check tag does not already exist',
      'Create annotated tag',
      'Push tag',
    ]) {
      const step = tag.jobs['tag-release'].steps.find(step => step.name === stepName)
      const script = step.run.replaceAll('${{ steps.extract.outputs.version }}', version)
      execFileSync('bash', ['-e', '-o', 'pipefail', '-c', script], {
        cwd: repo,
        stdio: 'pipe',
        env: {
          ...process.env,
          PR_HEAD_REF: `release/v${version}`,
          GITHUB_OUTPUT: output,
          HUSKY: '0',
        },
      })
    }
    assert.equal(
      git(origin, 'rev-parse', `refs/tags/v${version}^{}`),
      git(repo, 'rev-parse', 'HEAD')
    )
    git(source, 'push', 'origin', `:refs/tags/v${version}`)
    cases.push({ name, base, result: 'passed' })
  }
  report.result = 'passed'
} catch (error) {
  report.result = 'failed'
  report.error = String(error)
  throw error
} finally {
  fs.writeFileSync(artifact, `${JSON.stringify(report, null, 2)}\n`)
  fs.rmSync(temp, { recursive: true, force: true })
  console.log(artifact)
}
