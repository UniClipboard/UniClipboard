import { createHash } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const profile = process.argv[2]
if (!profile?.startsWith('dev-upgrade-')) throw new Error('Expected isolated upgrade profile')
const root = path.join(os.homedir(), 'Library/Application Support', `app.uniclipboard.desktop-${profile}`)
const output = path.resolve('tests/e2e/fixtures/upgrades/v1.0.0-alpha.5/macos-aarch64/single-node-text-link')
const stage = fs.mkdtempSync(path.join(os.tmpdir(), 'alpha5-baseline-'))
const hash = bytes => createHash('sha256').update(bytes).digest('hex')
const files = []
function collect(relative) {
  const source = path.join(root, relative)
  if (!fs.existsSync(source)) return
  const stat = fs.lstatSync(source)
  if (stat.isDirectory()) {
    for (const name of fs.readdirSync(source).sort()) collect(path.join(relative, name))
    return
  }
  if (!stat.isFile()) throw new Error('Unexpected non-regular fixture file')
  let bytes = fs.readFileSync(source)
  if (relative === 'settings.json') {
    const settings = JSON.parse(bytes)
    settings.general.telemetry_enabled = false
    settings.general.usage_analytics_enabled = false
    bytes = Buffer.from(JSON.stringify(settings, null, 2) + '\n')
  }
  const target = path.join(stage, 'data', relative)
  fs.mkdirSync(path.dirname(target), { recursive: true })
  fs.writeFileSync(target, bytes, { mode: 0o600 })
  files.push({ path: `data/${relative}`, size: bytes.length, sha256: hash(bytes) })
}
for (const item of ['uniclipboard.db', 'uniclipboard.db-wal', 'settings.json', 'upgrade-cursor.json', 'vault/.setup_status', 'vault/keyslot.json', 'vault/device_id.txt', 'keyring', 'iroh-identity', 'iroh-blobs']) collect(item)
fs.mkdirSync(output, { recursive: true })
execFileSync('tar', ['-czf', path.join(output, 'userdata.tar.gz'), '-C', stage, 'data'])
const archive = fs.readFileSync(path.join(output, 'userdata.tar.gz'))
fs.writeFileSync(path.join(output, 'manifest.json'), JSON.stringify({
  formatVersion: 1, sourceVersion: '1.0.0-alpha.5',
  sourceAssetSha256: 'a47e9b3c25002d4ec89e13ca61e463abeff04bab53d7abf465a44429e6a9a7c2',
  platform: 'macos-aarch64', scenario: 'single-node-text-link', environment: 'development',
  archive: 'userdata.tar.gz', archiveSha256: hash(archive), files,
}, null, 2) + '\n')
fs.writeFileSync(path.join(output, 'expected.json'), JSON.stringify({
  passphrase: 'upgrade-fixture-passphrase',
  entries: [
    { id: '41d4bede-bfb2-41f4-82d3-bfa58c78b485', text: 'Alpha5 upgrade baseline: first clipboard record' },
    { id: 'a4a6afce-e7a9-497c-b933-621c81767b40', text: 'Alpha5 upgrade baseline: second record\nSecond line' },
    { id: '629c0625-8f0c-4fe2-a3fb-2b48a8f383c7', text: 'https://example.com/alpha5-upgrade-baseline' },
  ],
}, null, 2) + '\n')
fs.rmSync(stage, { recursive: true })
console.log(JSON.stringify({ output, files: files.length, archiveBytes: archive.length, sha256: hash(archive) }))
