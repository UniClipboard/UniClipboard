import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createHash, randomUUID } from 'node:crypto'
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'

const fixture = path.resolve('tests/e2e/fixtures/upgrades/v1.0.0-alpha.5/macos-aarch64/single-node-text-link')
const oldDirectory = process.argv[2]
const currentDirectory = path.resolve(process.argv[3] ?? 'target/debug')
if (!oldDirectory) throw new Error('Pass the verified alpha.5 binary directory')
const profile = `dev-upgrade-alpha5-replay-${randomUUID()}`
const root = path.join(os.homedir(), 'Library/Application Support', `app.uniclipboard.desktop-${profile}`)
const reportDirectory = path.resolve('.planning/alpha5-upgrade', profile)
fs.mkdirSync(reportDirectory, { recursive: true })
const manifest = JSON.parse(fs.readFileSync(path.join(fixture, 'manifest.json')))
const expected = JSON.parse(fs.readFileSync(path.join(fixture, 'expected.json')))
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex')
const archive = path.join(fixture, manifest.archive)
if (sha256(fs.readFileSync(archive)) !== manifest.archiveSha256) throw new Error('Archive checksum mismatch')
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'alpha5-replay-'))
execFileSync('tar', ['-xzf', archive, '-C', stage])
for (const file of manifest.files) {
  const bytes = fs.readFileSync(path.join(stage, file.path))
  if (bytes.length !== file.size || sha256(bytes) !== file.sha256) throw new Error(`File checksum mismatch: ${file.path}`)
}
fs.cpSync(path.join(stage, 'data'), root, { recursive: true, errorOnExist: true, force: false })
fs.rmSync(stage, { recursive: true })
const env = { ...process.env, UC_PROFILE: profile, UNICLIPBOARD_ENV: 'development', UC_DAEMON_RUN_MODE: 'server', RUST_LOG: 'info' }
const results = []
let active
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
function start(directory, label, clipboard = false) {
  const fd = fs.openSync(path.join(reportDirectory, `${label}.log`), 'w')
  const childEnv = { ...env }
  if (clipboard) delete childEnv.UC_DAEMON_RUN_MODE
  active = spawn(path.join(directory, 'uniclipd'), [], { env: childEnv, stdio: ['ignore', fd, fd] })
  fs.closeSync(fd)
  return active
}
async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return
  child.kill('SIGTERM')
  for (let i = 0; i < 100 && child.exitCode === null && child.signalCode === null; i++) await pause(100)
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGKILL')
    await once(child, 'exit')
    throw new Error('Daemon did not stop gracefully')
  }
}
async function oldRun(label, clipboard = false) {
  const child = start(oldDirectory, label, clipboard)
  for (let i = 0; i < 200; i++) {
    if (child.exitCode !== null) throw new Error('Old daemon failed')
    if (fs.existsSync(path.join(root, 'daemon.conn'))) break
    await pause(100)
  }
  await pause(1000)
  const cli = path.join(oldDirectory, 'uniclip')
  const list = JSON.parse(execFileSync(cli, ['--json', 'get', '--list'], { env, timeout: 15000, encoding: 'utf8' }))
  if (list.length !== expected.entries.length) throw new Error('Unexpected history count')
  for (const entry of expected.entries) {
    const content = execFileSync(cli, ['get', '--id', entry.id], { env, timeout: 15000, encoding: 'utf8' }).trimEnd()
    if (content !== entry.text) throw new Error(`Content mismatch: ${entry.id}`)
  }
  if (clipboard) {
    const text = `Alpha5 clipboard written after failed upgrade ${profile}`
    execFileSync('pbcopy', { input: text })
    await pause(1500)
    const content = execFileSync(cli, ['get'], { env, timeout: 15000, encoding: 'utf8' }).trimEnd()
    if (content !== text) throw new Error('Post-failure clipboard capture did not persist')
  }
  await stop(child)
  results.push({ label, entriesVerified: list.length, addedClipboard: clipboard, exitCode: child.exitCode })
}
async function currentRun(label) {
  const child = start(currentDirectory, label)
  for (let i = 0; i < 300 && child.exitCode === null; i++) await pause(100)
  if (child.exitCode === null) {
    await stop(child)
    throw new Error('Current version did not reproduce startup failure')
  }
  const log = fs.readFileSync(path.join(reportDirectory, `${label}.log`), 'utf8').replace(/\x1b\[[0-9;]*m/g, '')
  const type = log.includes('"source_changed"') ? 'source_changed' : log.includes('"corrupt"') ? 'corrupt' : 'unknown'
  results.push({ label, exitCode: child.exitCode, errorType: type })
}
try {
  await oldRun('01-old-baseline-verification')
  await currentRun('02-first-upgrade')
  await currentRun('03-direct-retry')
  await oldRun('04-old-reopened', true)
  await currentRun('05-upgrade-after-old')
  await currentRun('06-repeat-after-source-changed')
  const failures = results.filter(result => result.errorType).map(result => result.errorType)
  if (JSON.stringify(failures) !== JSON.stringify(['corrupt', 'corrupt', 'source_changed', 'source_changed'])) {
    throw new Error(`Unexpected failure sequence: ${JSON.stringify(failures)}`)
  }
} finally {
  if (active) await stop(active)
  fs.writeFileSync(path.join(reportDirectory, 'result.json'), JSON.stringify({ profile, baselineSha256: manifest.archiveSha256, results }, null, 2) + '\n')
  console.log(JSON.stringify({ reportDirectory, results }, null, 2))
}
