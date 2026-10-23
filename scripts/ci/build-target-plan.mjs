import { appendFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'
import { selectBuildPlatforms } from './select-build-platforms.mjs'

// One authority for build, packaging and native acceptance targets.
export function buildTargetPlan(
  platform,
  { windowsRunner = 'windows-latest', signpathProduction = false } = {}
) {
  const targets = {
    'macos-aarch64': {
      platform: 'macos-latest',
      target: 'aarch64-apple-darwin',
      args: '--target aarch64-apple-darwin',
    },
    'macos-x86_64': {
      platform: 'macos-latest',
      target: 'x86_64-apple-darwin',
      args: '--target x86_64-apple-darwin',
    },
    'windows-x86_64': {
      platform: 'windows-latest',
      runner: windowsRunner,
      target: 'x86_64-pc-windows-msvc',
      args: '',
    },
    'windows-arm64': {
      platform: 'windows-latest',
      runner: windowsRunner,
      target: 'aarch64-pc-windows-msvc',
      args: '--target aarch64-pc-windows-msvc',
    },
    'ubuntu-22.04': {
      platform: 'ubuntu-22.04',
      target: 'x86_64-unknown-linux-gnu',
      args: '',
      container: 'ghcr.io/uniclipboard/build-bookworm:latest',
    },
    'ubuntu-22.04-arm': {
      platform: 'ubuntu-22.04-arm',
      target: 'aarch64-unknown-linux-gnu',
      args: '',
      container: 'ghcr.io/uniclipboard/build-bookworm:latest',
    },
  }
  if (platform !== 'linux-gui' && !targets[platform])
    throw new Error(`Unknown build platform: ${platform}`)
  const matrix = targets[platform] ? [targets[platform]] : []
  const macos = matrix.filter(e => e.platform === 'macos-latest')
  const windowsHosts = {
    'x86_64-pc-windows-msvc': { arch: 'amd64', smoke: 'windows-latest' },
    'aarch64-pc-windows-msvc': { arch: 'arm64', smoke: 'windows-11-arm' },
  }
  return {
    matrix,
    components: matrix.flatMap(e =>
      (e.target.includes('linux') ? ['daemon'] : ['daemon', 'helper']).map(component => ({
        ...e,
        component,
      }))
    ),
    cli: matrix.filter(
      e =>
        e.platform === 'macos-latest' ||
        (e.target === 'x86_64-pc-windows-msvc' && !signpathProduction)
    ),
    macos,
    macosSmoke: macos.map(e => ({
      target: e.target,
      ...(e.target.startsWith('aarch64')
        ? { arch: 'arm64', runner: 'macos-latest' }
        : { arch: 'x86_64', runner: 'macos-15-intel' }),
    })),
    windows: matrix
      .filter(e => windowsHosts[e.target])
      .map(e => ({ target: e.target, ...windowsHosts[e.target] })),
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const platforms = selectBuildPlatforms(process.env.UC_PLATFORM || 'all', {
    buildMode: process.env.UC_BUILD_MODE || 'release',
    buildOnly: process.env.UC_BUILD_ONLY === 'true',
  })
  const include = platforms.map(platform => ({
    platform,
    plan: JSON.stringify(
      buildTargetPlan(platform, {
        windowsRunner: process.env.WINDOWS_RUNNER || 'windows-latest',
        signpathProduction: process.env.SIGNPATH_PRODUCTION === 'true',
      })
    ),
  }))
  appendFileSync(process.env.GITHUB_OUTPUT, `matrix=${JSON.stringify({ include })}\n`)
}
