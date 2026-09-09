import { execFileSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

// Leave room for a Windows release cache upload before GitHub starts evicting entries.
const CACHE_BUDGET_BYTES = 8 * 1024 ** 3
const WINDOWS_APP_CACHE = /^v\d+-rust-x86_64-pc-windows-msvc(-test)?-Windows_NT-x64-/

function windowsCacheKind(key) {
  const match = key.match(WINDOWS_APP_CACHE)
  return match ? (match[1] ? 'test' : 'release') : undefined
}

function cacheFamily(key) {
  return (
    key.match(/^(v\d+-rust-.+-[a-f0-9]{8})-[a-f0-9]{8}$/)?.[1] ??
    key.match(/^((?:Linux|macOS|Windows)(?:-docs)?-bun)-[a-f0-9]{64}$/)?.[1] ??
    key.match(/^(codeql-dependencies-.+)-[a-f0-9]{64}$/)?.[1]
  )
}

export function planCacheCleanup(caches, openPullNumbers, defaultRef = 'refs/heads/main') {
  const openPulls = new Set(openPullNumbers)
  const remove = []
  const removed = new Set()
  let remainingBytes = caches.reduce((total, entry) => total + entry.size_in_bytes, 0)
  const select = (entry, reason) => {
    removed.add(entry.id)
    remove.push({ ...entry, reason })
    remainingBytes -= entry.size_in_bytes
  }

  for (const entry of caches) {
    if (!cacheFamily(entry.key)) continue
    const pull = entry.ref.match(/^refs\/pull\/(\d+)\/(?:head|merge)$/)
    if (pull && /^v\d+-rust-coverage-/.test(entry.key)) {
      select(entry, 'obsolete PR coverage cache')
    } else if (pull && !openPulls.has(Number(pull[1]))) {
      select(entry, 'closed PR cache')
    }
  }

  const newestFirst = [...caches].sort(
    (a, b) => Date.parse(b.created_at) - Date.parse(a.created_at)
  )
  const seen = new Set()
  for (const entry of newestFirst) {
    const family = cacheFamily(entry.key)
    if (!family || removed.has(entry.id)) continue
    const group = `${entry.ref}:${family}`
    if (seen.has(group)) select(entry, 'superseded cache generation')
    else seen.add(group)
  }

  const protectedIds = new Set()
  for (const kind of ['release', 'test']) {
    const windows = newestFirst.filter(
      entry => !removed.has(entry.id) && windowsCacheKind(entry.key) === kind
    )
    const preferred = windows.find(entry => entry.ref === defaultRef) ?? windows[0]
    if (preferred) protectedIds.add(preferred.id)
  }
  const oldestAccessFirst = [...caches].sort(
    (a, b) => Date.parse(a.last_accessed_at) - Date.parse(b.last_accessed_at)
  )
  for (const entry of oldestAccessFirst) {
    if (remainingBytes <= CACHE_BUDGET_BYTES) break
    if (removed.has(entry.id) || protectedIds.has(entry.id) || !cacheFamily(entry.key)) continue
    select(entry, 'reserve upload capacity')
  }

  return { remove, remainingBytes, overBudget: remainingBytes > CACHE_BUDGET_BYTES }
}

function api(endpoint, extra = []) {
  return JSON.parse(execFileSync('gh', ['api', endpoint, ...extra], { encoding: 'utf8' }))
}

export function maintainCaches({ repository, apply = false }) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? ''))
    throw new Error('Set GH_REPO to owner/repository')
  const prefix = `repos/${repository}`
  const { default_branch: defaultBranch } = api(prefix)
  const caches = api(`${prefix}/actions/caches?per_page=100`, ['--paginate', '--slurp']).flatMap(
    page => page.actions_caches
  )
  const openPulls = api(`${prefix}/pulls?state=open&per_page=100`, [
    '--paginate',
    '--slurp',
  ]).flatMap(page => page.map(pull => pull.number))
  const plan = planCacheCleanup(caches, openPulls, `refs/heads/${defaultBranch}`)
  for (const entry of plan.remove) {
    console.log(
      `${apply ? 'DELETE' : 'WOULD DELETE'} ${entry.id} ${entry.ref} ${entry.key}: ${entry.reason}`
    )
    if (apply) {
      // Delete the immutable cache ID selected by the plan, never an entire key prefix.
      const result = execFileSync(
        'gh',
        ['api', '--method', 'DELETE', `${prefix}/actions/caches/${entry.id}`],
        { encoding: 'utf8' }
      )
      if (result.trim()) console.log(result.trim())
    }
  }
  console.log(
    `${apply ? 'Retained' : 'Would retain'} ${(plan.remainingBytes / 1024 ** 3).toFixed(2)} GiB; ${plan.remove.length} cache entries selected`
  )
  if (plan.overBudget)
    throw new Error('Unknown or protected caches exceed the budget; review them manually')
  return plan
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.slice(2).some(arg => arg !== '--apply'))
    throw new Error('Usage: maintain-actions-cache.mjs [--apply]')
  maintainCaches({ repository: process.env.GH_REPO, apply: process.argv.includes('--apply') })
}
