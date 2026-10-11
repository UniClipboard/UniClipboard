// Build the local Wails application using the existing platform build owners.
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
if (args.some(arg => arg !== '--release') || args.length > 1) {
  console.error('Usage: bun wails:build [--release]')
  process.exit(1)
}
const release = args.includes('--release')
if (release && process.platform !== 'win32') {
  console.error('The local release portable build currently supports Windows only.')
  process.exit(1)
}
const builds = {
  win32: {
    command: 'python',
    args: ['apps/gui-go/e2e/build_windows.py', '--mode', release ? 'release' : 'production'],
    output: release
      ? 'target/gui-go/windows-release/ (release portable ZIP)'
      : 'target/gui-go/windows-production/gui-go.exe',
  },
  darwin: {
    command: 'bash',
    args: ['apps/gui-go/build.sh', 'manual'],
    output: 'target/gui-go/UniClipboardGo.app',
  },
}

const build = builds[process.platform]
if (!build) {
  console.error('The local Wails build currently supports macOS and Windows.')
  process.exitCode = 1
} else {
  const result = spawnSync(build.command, build.args, {
    cwd: root,
    stdio: 'inherit',
    windowsHide: true,
  })
  if (result.error) console.error(`[wails-build] ${result.error.message}`)
  process.exitCode = result.status ?? 1
  if (process.exitCode === 0) console.log(`[wails-build] Built ${build.output}`)
}
