#!/usr/bin/env node
// Thin launcher for the platform-specific `uniclip` binary.
//
// The real binaries (`uniclip` + `uniclipd`) live in the matching
// @uniclipboard/cli-{platform}-{arch} optional dependency. We must spawn the
// binary by its real path inside that package — NOT symlink it through npm's
// `bin` machinery — because `uniclip start` resolves the `uniclipd` daemon as
// a sibling of `current_exe()` (ADR-008 D13). npm bin shims/symlinks would
// break that sibling lookup, especially on Windows where npm generates .cmd
// wrappers.

import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import path from 'node:path'
import process from 'node:process'

const PLATFORM_PACKAGES = {
  'darwin-arm64': '@uniclipboard/cli-darwin-arm64',
  'darwin-x64': '@uniclipboard/cli-darwin-x64',
  'linux-arm64': '@uniclipboard/cli-linux-arm64',
  'linux-x64': '@uniclipboard/cli-linux-x64',
  'win32-x64': '@uniclipboard/cli-win32-x64',
}

function resolveBinary() {
  const key = `${process.platform}-${process.arch}`
  const pkg = PLATFORM_PACKAGES[key]

  if (!pkg) {
    console.error(
      `uniclipboard: unsupported platform "${key}".\n` +
        `Supported platforms: ${Object.keys(PLATFORM_PACKAGES).join(', ')}.\n` +
        `Prebuilt binaries are also available at ` +
        `https://github.com/UniClipboard/UniClipboard/releases`
    )
    process.exit(1)
  }

  const require = createRequire(import.meta.url)
  let pkgDir
  try {
    pkgDir = path.dirname(require.resolve(`${pkg}/package.json`))
  } catch {
    console.error(
      `uniclipboard: platform package "${pkg}" is not installed.\n` +
        `This usually means optional dependencies were skipped ` +
        `(e.g. --no-optional / --omit=optional) or the package manager ` +
        `cache is stale.\n` +
        `Try reinstalling: npm install uniclipboard`
    )
    process.exit(1)
  }

  const exe = process.platform === 'win32' ? 'uniclip.exe' : 'uniclip'
  return path.join(pkgDir, 'bin', exe)
}

const binary = resolveBinary()
const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' })

if (result.error) {
  console.error(`uniclipboard: failed to run ${binary}: ${result.error.message}`)
  process.exit(1)
}
if (result.signal) {
  // Propagate the child's fatal signal so callers see the same termination.
  process.kill(process.pid, result.signal)
}
process.exit(result.status ?? 1)
