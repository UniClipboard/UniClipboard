// Build and serve the gallery for manual inspection. Automated acceptance uses e2e:gui layouts ui-kit.
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { allocateBuildLease, releaseBuildLease, buildLeaseEnv } from './tester-army/build-lease.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const frontend = path.resolve(here, '..')
const args = process.argv.slice(2)
const option = name => {
  const index = args.indexOf(name)
  return index >= 0 ? args[index + 1] : undefined
}
const platform = option('--platform')
// A present --port needs a valid value; only an absent flag falls back to the default.
const port = args.includes('--port') ? option('--port') : '1460'
if (!/^[1-9][0-9]{0,4}$/.test(port ?? '') || Number(port) > 65535) {
  console.error(
    `--port needs a TCP port number (got ${port === undefined ? 'nothing' : `"${port}"`})`
  )
  process.exit(2)
}
if (!['windows', 'macos', 'linux'].includes(platform ?? '')) {
  console.error('Usage: node e2e/ui-kit-preview.mjs --platform windows|macos|linux [--port 1460]')
  process.exit(2)
}

const lease = await allocateBuildLease()
const server = spawn(process.execPath, ['e2e/tester-army/server.mjs', port], {
  cwd: frontend,
  env: {
    ...process.env,
    [buildLeaseEnv]: lease,
    UC_UI_PLATFORM: platform,
    UC_TESTER_FIXTURE: 'ui-kit-gallery',
    UC_TESTER_DAEMON: '0',
    UC_TESTER_BACKENDS: '0',
  },
  stdio: 'inherit',
})
let interrupted
for (const signal of ['SIGINT', 'SIGTERM'])
  process.on(signal, () => {
    if (interrupted) return
    interrupted = signal
    if (server.exitCode === null && server.signalCode === null) server.kill(signal)
  })
let failure
server.once('error', error => {
  failure = error
})
const code = await new Promise(resolve => server.once('close', resolve))
const cleanup = await releaseBuildLease(lease)
console.log(JSON.stringify({ event: 'preview-cleanup', ...cleanup }))
if (failure) throw failure
process.exitCode = !cleanup.passed
  ? 1
  : interrupted === 'SIGINT'
    ? 130
    : interrupted === 'SIGTERM'
      ? 143
      : (code ?? 1)
