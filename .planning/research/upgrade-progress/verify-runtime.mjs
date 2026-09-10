import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'

const fixture = path.resolve('tests/e2e/fixtures/upgrades/v1.0.0-alpha.5/macos-aarch64/single-node-text-link')
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'uc-startup-runtime-'))
const profile = `dev-startup-${randomUUID()}`
const manifest = JSON.parse(fs.readFileSync(path.join(fixture, 'manifest.json')))
const hash = bytes => createHash('sha256').update(bytes).digest('hex')
assert.equal(hash(fs.readFileSync(path.join(fixture, manifest.archive))), manifest.archiveSha256)
const stage = path.join(work, 'baseline')
fs.mkdirSync(stage)
execFileSync('tar', ['-xzf', path.join(fixture, manifest.archive), '-C', stage])
for (const file of manifest.files) assert.equal(hash(fs.readFileSync(path.join(stage, file.path))), file.sha256)
const root = path.join(work, 'data', `app.uniclipboard.desktop-${profile}`)
fs.cpSync(path.join(stage, 'data'), root, { recursive: true })
fs.copyFileSync(path.resolve('target/debug/uniclipd'), path.join(work, 'uniclipd'))
fs.chmodSync(path.join(work, 'uniclipd'), 0o700)
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
const results = []
const expected = JSON.parse(fs.readFileSync(path.join(fixture, 'expected.json')))
for (const phase of ['upgrade', 'restart']) {
  const fd = fs.openSync(path.join(work, `${phase}.log`), 'w')
  const child = spawn(path.join(work, 'uniclipd'), [], { env: { ...process.env, UC_PROFILE: profile,
    UC_PORTABLE: '1', UC_DAEMON_RUN_MODE: 'server', RUST_LOG: 'warn' }, stdio: ['ignore', fd, fd] })
  fs.closeSync(fd)
  child.on('error', error => { throw error })
  try {
    const seen = new Set()
    let ready = false
    let early = false
    for (let i = 0; i < 6000; i++) {
      if (child.exitCode !== null) throw new Error(`Daemon exited; inspect ${work}`)
      const connPath = path.join(root, 'daemon-startup.conn')
      if (fs.existsSync(connPath)) {
        const conn = JSON.parse(fs.readFileSync(connPath))
        const url = `http://127.0.0.1:${conn.port}/startup`
        const response = await fetch(url, { headers: { Authorization: `Bearer ${conn.token}` } })
        assert.equal(response.status, 200)
        const status = await response.json()
        seen.add(status.progress.state)
        if (!status.service_ready) early = true
        if (status.progress.state === 'failed' || status.service_failed) throw new Error(`Startup failed; inspect ${work}`)
        if (status.service_ready) {
          assert.equal(status.progress.state, 'ready')
          const business = JSON.parse(fs.readFileSync(path.join(root, 'daemon.conn')))
          const health = await fetch(`http://127.0.0.1:${business.port}/health`)
          assert.equal(health.status, 200)
          const base = `http://127.0.0.1:${business.port}`
          const auth = await fetch(`${base}/auth/connect`, { method: 'POST', headers: {
            Authorization: `Bearer ${business.token}`, 'Content-Type': 'application/json',
          }, body: JSON.stringify({ pid: process.pid, clientType: 'cli' }) })
          assert.equal(auth.status, 200)
          const session = (await auth.json()).data.sessionToken
          const entries = await fetch(`${base}/clipboard/entries?limit=50`, { headers: { Authorization: `Session ${session}` } })
          assert.equal(entries.status, 200)
          assert.equal((await entries.json()).data.length, expected.entries.length)
          assert.equal((await fetch(url)).status, 401)
          ready = true
          break
        }
      }
      await pause(10)
    }
    assert.equal(ready, true)
    results.push({ phase, statusBeforeBusinessReady: early, states: [...seen], businessHealthy: ready, historyEntries: expected.entries.length })
  } finally {
    child.kill('SIGTERM')
    for (let i = 0; i < 300 && child.exitCode === null && child.signalCode === null; i++) await pause(100)
    if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); throw new Error('Daemon did not exit gracefully') }
  }
}
fs.writeFileSync(path.join(work, 'result.json'), JSON.stringify(results, null, 2))
console.log(JSON.stringify({ work, results }))
