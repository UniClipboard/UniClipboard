import fs from 'node:fs'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { parseSemver } from './bump-version-lib.js'

const PRERELEASE_ORDER = {
  alpha: 0,
  beta: 1,
  rc: 2,
}

export function stripVersionTagPrefix(value) {
  return value.startsWith('v') ? value.slice(1) : value
}

function comparePrerelease(a, b) {
  if (a.prerelease === b.prerelease) {
    if (a.prerelease === null) {
      return 0
    }
    return a.prereleaseVersion - b.prereleaseVersion
  }

  if (a.prerelease === null) {
    return 1
  }

  if (b.prerelease === null) {
    return -1
  }

  const aOrder = PRERELEASE_ORDER[a.prerelease] ?? Number.MAX_SAFE_INTEGER
  const bOrder = PRERELEASE_ORDER[b.prerelease] ?? Number.MAX_SAFE_INTEGER

  if (aOrder !== bOrder) {
    return aOrder - bOrder
  }

  return a.prerelease.localeCompare(b.prerelease)
}

export function compareVersions(aVersion, bVersion) {
  const a = parseSemver(stripVersionTagPrefix(aVersion))
  const b = parseSemver(stripVersionTagPrefix(bVersion))

  if (a.major !== b.major) {
    return a.major - b.major
  }

  if (a.minor !== b.minor) {
    return a.minor - b.minor
  }

  if (a.patch !== b.patch) {
    return a.patch - b.patch
  }

  return comparePrerelease(a, b)
}

function normalizeRelease(release) {
  return {
    tagName: release.tagName ?? release.tag_name,
    isDraft: release.isDraft ?? release.draft ?? false,
    isPrerelease: release.isPrerelease ?? release.prerelease ?? false,
    publishedAt: release.publishedAt ?? release.published_at ?? null,
    version: stripVersionTagPrefix(release.tagName ?? release.tag_name),
  }
}

export function selectPreviousPublishedRelease(releases, currentVersion) {
  const normalizedCurrentVersion = stripVersionTagPrefix(currentVersion)
  const currentIsStable = parseSemver(normalizedCurrentVersion).prerelease === null

  const candidates = releases
    .filter(release => !release.isDraft)
    .filter(release => Boolean(release.publishedAt))
    .map(normalizeRelease)
    .filter(release => compareVersions(release.version, normalizedCurrentVersion) < 0)
    .filter(release => !currentIsStable || parseSemver(release.version).prerelease === null)
    .sort((left, right) => compareVersions(right.version, left.version))

  return candidates[0] ?? null
}

export async function fetchPublishedReleases(repo, token, fetchImpl = fetch) {
  const releases = []
  let page = 1

  while (true) {
    const response = await fetchImpl(
      `https://api.github.com/repos/${repo}/releases?per_page=100&page=${page}`,
      {
        headers: {
          Accept: 'application/vnd.github+json',
          Authorization: token ? `Bearer ${token}` : undefined,
          'User-Agent': 'uniclipboard-release-history',
        },
      }
    )

    if (!response.ok) {
      throw new Error(
        `Failed to fetch releases for ${repo}: ${response.status} ${response.statusText}`
      )
    }

    const pageItems = await response.json()
    releases.push(...pageItems.map(normalizeRelease))

    if (pageItems.length < 100) {
      break
    }

    page += 1
  }

  return releases
}

function parseArgs(argv) {
  const args = {}

  for (let index = 0; index < argv.length; index += 1) {
    const current = argv[index]
    const next = argv[index + 1]

    if (!current.startsWith('--')) {
      continue
    }

    const key = current.slice(2)
    args[key] = next
    index += 1
  }

  return args
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const repo = args.repo || process.env.GITHUB_REPOSITORY
  const currentVersion = args['current-version']

  if (!repo) {
    throw new Error('Missing --repo or GITHUB_REPOSITORY')
  }

  if (!currentVersion) {
    throw new Error('Missing --current-version')
  }

  const token =
    process.env.GITHUB_TOKEN || process.env.GH_TOKEN || process.env.REPO_BOT_TOKEN || undefined
  const releases = await fetchPublishedReleases(repo, token)
  const previousRelease = selectPreviousPublishedRelease(releases, currentVersion)
  const payload = {
    previousTag: previousRelease?.tagName ?? '',
    previousVersion: previousRelease?.version ?? '',
  }

  if (args['github-output']) {
    const lines = [
      `previous_tag=${payload.previousTag}`,
      `previous_version=${payload.previousVersion}`,
    ]
    fs.appendFileSync(args['github-output'], `${lines.join('\n')}\n`)
    return
  }

  process.stdout.write(`${JSON.stringify(payload, null, 2)}\n`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  })
}
