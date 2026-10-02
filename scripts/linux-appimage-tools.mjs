// Pin the linuxdeploy that Tauri's AppImage bundler runs.
//
// tauri-cli 2.11.x downloads an unpinned, 2024-era `linuxdeploy` into the user
// cache directory and only does so when no file of that name exists. That
// build predates the community exclude list entry for libwayland-client.so.0,
// so it copies the build host's libwayland-client into the AppImage. At run
// time the bundled copy shadows the host one for every process, including the
// host Mesa EGL driver that needs a newer libwayland-client, and the WebView
// fails to initialize EGL (see docs/architecture/linux-appimage-library-policy.md).
//
// Seeding the cache with a pinned, checksum-verified linuxdeploy whose exclude
// list contains libwayland-client.so.0 fixes the bundle without touching the
// rest of the packaging toolchain.
import { createHash } from 'node:crypto'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

const RELEASE_BASE = 'https://github.com/tauri-apps/binary-releases/releases/download'

// Same linuxdeploy build tauri-bundler's development branch pins
// (LINUXDEPLOY_COMMIT_HASH in crates/tauri-bundler/.../appimage/linuxdeploy.rs).
export const PINNED_LINUXDEPLOY = {
  release: 'linuxdeploy-07333c6',
  sha256: {
    x86_64: '36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62',
    aarch64: '556ab80baa98e600aa80f0dcedfb70bca0e1ce7e9f147fb345be3fcc3e91b2b1',
  },
}

// Mirrors `dirs::cache_dir().join("tauri")`, the default tools directory of tauri-bundler.
export function tauriToolsDirectory(env = process.env) {
  const cache = env.XDG_CACHE_HOME || join(env.HOME || homedir(), '.cache')
  return join(cache, 'tauri')
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex')
}

export async function installPinnedLinuxdeploy({
  arch,
  env = process.env,
  fetchImpl = fetch,
  pinned = PINNED_LINUXDEPLOY,
}) {
  const expected = pinned.sha256[arch]
  if (!expected) throw new Error(`No pinned linuxdeploy for architecture: ${arch}`)

  const tools = tauriToolsDirectory(env)
  const verified = join(tools, 'pinned', `${pinned.release}-${arch}.AppImage`)
  const target = join(tools, `linuxdeploy-${arch}.AppImage`)
  mkdirSync(join(tools, 'pinned'), { recursive: true })

  if (!existsSync(verified) || sha256(readFileSync(verified)) !== expected) {
    const url = `${RELEASE_BASE}/${pinned.release}/linuxdeploy-${arch}.AppImage`
    const response = await fetchImpl(url)
    if (!response.ok) throw new Error(`Failed to download ${url}: HTTP ${response.status}`)
    const bytes = Buffer.from(await response.arrayBuffer())
    const actual = sha256(bytes)
    if (actual !== expected) {
      throw new Error(
        `Unexpected linuxdeploy checksum for ${arch}: ${actual} (expected ${expected})`
      )
    }
    writeFileSync(`${verified}.partial`, bytes)
    renameSync(`${verified}.partial`, verified)
  }

  // tauri-bundler zeroes bytes of the AppImage magic in place after it picks the
  // file up, so it always receives a fresh copy of the verified original.
  copyFileSync(verified, `${target}.partial`)
  chmodSync(`${target}.partial`, 0o755)
  renameSync(`${target}.partial`, target)
  return target
}
