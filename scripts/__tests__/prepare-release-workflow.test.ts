import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { parse } from 'yaml'

const workflow = parse(
  fs.readFileSync(
    path.resolve(import.meta.dirname, '../../.github/workflows/prepare-release.yml'),
    'utf8'
  )
)
const sandboxes: string[] = []
const version = '1.2.3-alpha.1'
const branch = `release/v${version}`

function git(cwd: string, ...args: string[]) {
  return execFileSync('git', args, {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim()
}

function sandbox() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'prepare-release-'))
  sandboxes.push(root)
  const origin = path.join(root, 'origin.git')
  const repo = path.join(root, 'prepare')
  git(root, 'init', '--bare', origin)
  git(root, 'clone', origin, repo)
  git(repo, 'config', 'user.name', 'Release test')
  git(repo, 'config', 'user.email', 'release@example.test')
  fs.writeFileSync(path.join(repo, 'version.txt'), '1.2.2\n')
  git(repo, 'add', '.')
  git(repo, 'commit', '-m', 'Initial version')
  const base = git(repo, 'rev-parse', 'HEAD')
  git(repo, 'tag', 'v1.2.2')
  git(repo, 'push', 'origin', 'HEAD:main', '--tags')
  git(repo, 'checkout', '-b', branch)
  fs.writeFileSync(path.join(repo, 'version.txt'), `${version}\n`)
  git(repo, 'commit', '-am', 'Bump version')
  return { root, repo, origin, base }
}

function step(job: string, name: string): string {
  const result = workflow.jobs[job].steps.find((entry: { name?: string }) => entry.name === name)
  if (!result?.run) throw new Error(`Missing executable step ${job}/${name}`)
  return result.run
}

function run(cwd: string, script: string, env: Record<string, string>) {
  return execFileSync('bash', ['--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', script], {
    cwd,
    env: { ...process.env, HUSKY: '0', ...env },
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  })
}

afterEach(() => {
  for (const root of sandboxes.splice(0)) fs.rmSync(root, { recursive: true, force: true })
})

describe('prepare release job recovery', () => {
  it.each([false, true])('restores every checkpoint with formatting changes=%s', formatted => {
    const { root, origin, base } = sandbox()
    let repo = path.join(root, 'prepare')
    for (const [producer, consumer] of [
      ['prepare', 'format'],
      ['format', 'changelog'],
      ['changelog', 'publish'],
    ]) {
      const artifact = path.join(root, `${producer}-artifact`)
      fs.mkdirSync(artifact)
      run(repo, step(producer, 'Save release checkpoint'), {
        RUNNER_TEMP: artifact,
        BASE_SHA: base,
      })
      const runner = path.join(root, consumer)
      const download = path.join(runner, 'release-checkpoint')
      fs.mkdirSync(download, { recursive: true })
      fs.copyFileSync(path.join(artifact, 'release.bundle'), path.join(download, 'release.bundle'))
      const nextRepo = path.join(runner, 'repo')
      git(root, 'clone', origin, nextRepo)
      git(nextRepo, 'checkout', base)
      run(nextRepo, step(consumer, 'Restore release branch'), {
        RUNNER_TEMP: runner,
        VERSION: version,
      })
      expect(git(nextRepo, 'rev-parse', 'HEAD')).toBe(git(repo, 'rev-parse', 'HEAD'))
      expect(git(nextRepo, 'branch', '--show-current')).toBe(branch)
      expect(fs.readFileSync(path.join(nextRepo, 'version.txt'), 'utf8')).toBe(`${version}\n`)
      expect(git(nextRepo, 'rev-parse', 'v1.2.2')).toBe(base)
      repo = nextRepo
      if (consumer === 'format' && formatted) {
        fs.writeFileSync(path.join(repo, 'version.txt'), `${version}\n\n`)
        git(repo, 'commit', '-am', 'Format version')
        // Restore the expected content while preserving a distinct formatting commit.
        fs.writeFileSync(path.join(repo, 'version.txt'), `${version}\n`)
        git(repo, 'commit', '-am', 'Normalize version')
      }
      if (consumer === 'changelog') {
        fs.writeFileSync(path.join(repo, 'changelog.md'), 'Final changelog\n')
        git(repo, 'add', '.')
        git(repo, 'commit', '-m', 'Polish changelog')
      }
    }
    expect(fs.readFileSync(path.join(repo, 'changelog.md'), 'utf8')).toBe('Final changelog\n')
    expect(git(repo, 'status', '--porcelain')).toBe('')
  })

  it('allows publishing the same checkpoint twice and rejects a changed remote branch', () => {
    const { repo, origin } = sandbox()
    const publish = () => run(repo, step('publish', 'Push release branch'), { VERSION: version })
    publish()
    publish()
    expect(git(repo, 'rev-parse', 'HEAD')).toBe(git(origin, 'rev-parse', `refs/heads/${branch}`))
    fs.writeFileSync(path.join(repo, 'manual.txt'), 'Manual edit\n')
    git(repo, 'add', '.')
    git(repo, 'commit', '-m', 'Manual edit')
    git(repo, 'push', 'origin', branch)
    const remoteHead = git(origin, 'rev-parse', `refs/heads/${branch}`)
    git(repo, 'reset', '--hard', 'HEAD~1')
    expect(publish).toThrow(/refusing to overwrite/)
    expect(git(origin, 'rev-parse', `refs/heads/${branch}`)).toBe(remoteHead)
  })

  it('reuses a PR when creation succeeded before the job was retried', () => {
    const { root, repo } = sandbox()
    const bin = path.join(root, 'bin')
    fs.mkdirSync(bin)
    fs.writeFileSync(
      path.join(bin, 'gh'),
      `#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  'pr list')
    if [ -f "$RUNNER_TEMP/pr-created" ]; then cat "$RUNNER_TEMP/pr-created"; fi
    ;;
  'pr create')
    test ! -f "$RUNNER_TEMP/pr-created"
    grep -q 'Release v1.2.3-alpha.1' "$RUNNER_TEMP/release-pr-body.md"
    echo 'https://github.com/example/repo/pull/1' | tee "$RUNNER_TEMP/pr-created"
    ;;
  *) exit 1 ;;
esac
`,
      { mode: 0o755 }
    )
    const env = {
      VERSION: version,
      BASE_BRANCH: 'main',
      RUNNER_TEMP: root,
      GITHUB_ENV: path.join(root, 'github-env'),
      GITHUB_OUTPUT: path.join(root, 'github-output'),
      PATH: `${bin}${path.delimiter}${process.env.PATH}`,
    }
    run(repo, step('publish', 'Create pull request'), env)
    run(repo, step('publish', 'Create pull request'), env)
    expect(fs.readFileSync(env.GITHUB_ENV, 'utf8').trim().split('\n')).toEqual([
      'PR_URL=https://github.com/example/repo/pull/1',
      'PR_URL=https://github.com/example/repo/pull/1',
    ])
  })
})
