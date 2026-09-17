// Ordered by the measured release critical path. Warmup and retention share this list.
export const releaseCacheTargets = [
  { platform: 'macos-aarch64', target: 'aarch64-apple-darwin' },
  { platform: 'windows-arm64', target: 'aarch64-pc-windows-msvc' },
  { platform: 'windows-x86_64', target: 'x86_64-pc-windows-msvc' },
]

export function appCachePriority(key) {
  const target = key.match(/^v\d+-rust-(.+?)-(?:Darwin|Windows_NT|Linux)-/)?.[1]
  const release = releaseCacheTargets.findIndex(entry => entry.target === target)
  if (release !== -1) return release
  if (target === 'x86_64-pc-windows-msvc-test') return releaseCacheTargets.length
  return undefined
}
