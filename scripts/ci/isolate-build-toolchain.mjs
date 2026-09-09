import { execFileSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

export function unusedToolchains(installed, active, channel) {
  if (!channel || !active.startsWith(`${channel}-`)) {
    throw new Error('The active toolchain does not match the repository pin')
  }
  if (!installed.includes(active)) throw new Error('The pinned active toolchain is not installed')
  return installed.filter(toolchain => toolchain !== active)
}

function installedToolchains() {
  return execFileSync('rustup', ['toolchain', 'list', '--quiet'], { encoding: 'utf8' })
    .trim()
    .split(/\s+/)
    .filter(Boolean)
}

export function isolateBuildToolchain(channel, env = process.env) {
  if (env.GITHUB_ACTIONS !== 'true' || env.RUNNER_ENVIRONMENT !== 'github-hosted') {
    throw new Error('Toolchain isolation is restricted to disposable GitHub-hosted runners')
  }
  const active = execFileSync('rustup', ['show', 'active-toolchain'], { encoding: 'utf8' })
    .trim()
    .split(/\s+/)[0]
  const unused = unusedToolchains(installedToolchains(), active, channel)
  if (unused.length) {
    // rust-cache hashes every installed compiler, including unused runner-image toolchains.
    execFileSync('rustup', ['toolchain', 'uninstall', ...unused], { stdio: 'inherit' })
  }
  const remaining = installedToolchains()
  if (remaining.length !== 1 || remaining[0] !== active) {
    throw new Error('Build runner still contains unexpected Rust toolchains')
  }
  console.log(`Build and cache use only ${active}`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  isolateBuildToolchain(process.env.UC_PINNED_RUST)
}
