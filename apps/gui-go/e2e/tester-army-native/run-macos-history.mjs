import assert from 'node:assert/strict'
import { spawn, execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFile, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../../../../', import.meta.url))
assert.equal(process.platform, 'darwin', 'native history acceptance requires macOS')
assert.ok(Number(process.versions.node.split('.')[0]) >= 24, 'TesterArmy requires Node 24 or later')
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim()
const head = git('rev-parse', 'HEAD')
const status = git('status', '--porcelain')

async function run(command, args) {
  const child = spawn(command, args, {
    cwd: root,
    env: { ...process.env, E2E_TELEMETRY_DISABLED: '1' },
    stdio: 'inherit',
  })
  const forward = signal => {
    if (child.exitCode === null) child.kill(signal)
  }
  const interrupt = () => forward('SIGINT')
  const terminate = () => forward('SIGTERM')
  process.once('SIGINT', interrupt)
  process.once('SIGTERM', terminate)
  try {
    const code = await new Promise((resolve, reject) => {
      child.once('error', reject)
      child.once('close', resolve)
    })
    assert.equal(code, 0, `${path.basename(command)} did not complete successfully`)
  } finally {
    process.removeListener('SIGINT', interrupt)
    process.removeListener('SIGTERM', terminate)
  }
}

await rm(path.join(root, 'target/tester-army-macos-history'), { recursive: true, force: true })
await run('bash', ['apps/gui-go/build.sh', 'e2e'])
assert.equal(git('rev-parse', 'HEAD'), head, 'source HEAD changed while building')
assert.equal(git('status', '--porcelain'), status, 'working-tree state changed while building')
const files = {
  host: 'target/gui-go/UniClipboardGoE2E.app/Contents/MacOS/gui-go',
  ui: 'target/gui-go/UniClipboardGoE2E.app/Contents/Helpers/UniClipboardUI.app/Contents/MacOS/gui-go',
  daemon: 'target/debug/uniclipd',
  cli: 'target/gui-go/uniclip',
}
const binariesSha256 = Object.fromEntries(
  await Promise.all(
    Object.entries(files).map(async ([name, file]) => [
      name,
      createHash('sha256')
        .update(await readFile(path.join(root, file)))
        .digest('hex'),
    ])
  )
)
await writeFile(
  path.join(root, 'target/gui-go/macos-native-build.json'),
  JSON.stringify(
    {
      head,
      dirty: status !== '',
      node: process.versions.node,
      platform: process.platform,
      arch: process.arch,
      binariesSha256,
    },
    null,
    2
  ) + '\n'
)
await run(process.execPath, [
  'node_modules/e2e/dist/cli/bin.js',
  'run',
  '--config',
  'e2e.macos-history.config.ts',
])
