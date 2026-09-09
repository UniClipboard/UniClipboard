import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createHash, randomUUID } from 'node:crypto'
import { execFileSync, spawn } from 'node:child_process'

const [oldDirectory, brokenDirectory, fixedDirectory = 'target/debug'] = process.argv.slice(2)
if (!oldDirectory || !brokenDirectory) throw new Error('Pass alpha.5 and pre-fix binary directories')
const fixture = path.resolve('tests/e2e/fixtures/upgrades/v1.0.0-alpha.5/macos-aarch64/single-node-text-link')
const expected = JSON.parse(fs.readFileSync(path.join(fixture, 'expected.json')))
const manifest = JSON.parse(fs.readFileSync(path.join(fixture, 'manifest.json')))
const archive = path.join(fixture, manifest.archive)
const hash = value => createHash('sha256').update(value).digest('hex')
if (hash(fs.readFileSync(archive)) !== manifest.archiveSha256) throw new Error('Baseline digest mismatch')
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
const results = []
for (const scenario of ['fresh', 'failed-upgrade', 'changed-source']) {
  const profile = `dev-upgrade-alpha5-repair-${scenario}-${randomUUID()}`
  const root = path.join(os.homedir(), 'Library/Application Support', `app.uniclipboard.desktop-${profile}`)
  const logs = path.resolve('.planning/alpha5-upgrade', profile)
  fs.mkdirSync(logs, { recursive: true })
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'alpha5-repair-'))
  execFileSync('tar', ['-xzf', archive, '-C', stage])
  for (const item of manifest.files) {
    const bytes = fs.readFileSync(path.join(stage, item.path))
    if (bytes.length !== item.size || hash(bytes) !== item.sha256) throw new Error('Baseline file mismatch')
  }
  fs.cpSync(path.join(stage, 'data'), root, { recursive: true, errorOnExist: true, force: false })
  fs.rmSync(stage, { recursive: true })
  const env = { ...process.env, UC_PROFILE: profile, UNICLIPBOARD_ENV: 'development', RUST_LOG: 'info' }
  let active
  const exited = child => child.exitCode !== null || child.signalCode !== null
  function start(directory, label, clipboard = false) {
    const fd = fs.openSync(path.join(logs, `${label}.log`), 'w')
    active = spawn(path.resolve(directory, 'uniclipd'), [], {
      env: { ...env, UC_DAEMON_RUN_MODE: clipboard ? '' : 'server' }, stdio: ['ignore', fd, fd],
    })
    active.on('error', error => { throw error })
    fs.closeSync(fd)
    return active
  }
  async function stop(child) {
    if (exited(child)) return
    child.kill('SIGTERM')
    for (let i = 0; i < 100 && !exited(child); i++) await pause(100)
    if (!exited(child)) {
      child.kill('SIGKILL')
      while (!exited(child)) await pause(100)
      throw new Error('Graceful stop timed out')
    }
  }
  async function ready(child) {
    for (let i = 0; i < 300; i++) {
      if (exited(child)) throw new Error(`Startup failed: ${scenario}; inspect ${logs}`)
      if (fs.existsSync(path.join(root, 'daemon.conn'))) { await pause(1000); return }
      await pause(100)
    }
    throw new Error('Startup timed out')
  }
  function cli(directory, args) {
    return execFileSync(path.resolve(directory, 'uniclip'), args, { env, timeout: 15000, encoding: 'utf8' })
  }
  async function broken(label, type) {
    const child = start(brokenDirectory, label)
    for (let i = 0; i < 300 && !exited(child); i++) await pause(100)
    if (!exited(child)) throw new Error('Pre-fix startup unexpectedly succeeded')
    const log = fs.readFileSync(path.join(logs, `${label}.log`), 'utf8')
    if (child.exitCode !== 1 || !log.includes(`"${type}"`)) throw new Error('Unexpected pre-fix error')
  }
  let additional
  try {
    if (scenario !== 'fresh') await broken('01-pre-fix', 'corrupt')
    if (scenario === 'changed-source') {
      const old = start(oldDirectory, '02-old-write', true)
      await ready(old)
      additional = 'Synthetic clipboard captured after failed upgrade'
      execFileSync('pbcopy', { input: additional })
      await pause(1500)
      if (cli(oldDirectory, ['get']).trimEnd() !== additional) throw new Error('Old capture missing')
      await stop(old)
      await broken('03-source-changed', 'source_changed')
    }
    for (const label of ['04-fixed-upgrade', '05-fixed-restart']) {
      const fixed = start(fixedDirectory, label)
      await ready(fixed)
      const list = JSON.parse(cli(fixedDirectory, ['--json', 'get', '--list']))
      if (list.length !== expected.entries.length + Number(Boolean(additional))) throw new Error('History count changed')
      for (const entry of expected.entries) {
        if (cli(fixedDirectory, ['get', '--id', entry.id]).trimEnd() !== entry.text) throw new Error('History content changed')
      }
      if (additional && cli(fixedDirectory, ['get']).trimEnd() !== additional) throw new Error('New old-version record was lost')
      results.push({ scenario, label, verifiedEntries: list.length })
      await stop(fixed)
    }
  } finally {
    if (active) await stop(active)
    fs.writeFileSync(path.join(logs, 'result.json'), JSON.stringify({ profile, results: results.filter(result => result.scenario === scenario) }, null, 2) + '\n')
  }
}
console.log(JSON.stringify({ results }, null, 2))
