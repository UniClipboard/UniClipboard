import semver from 'semver'
import type { Manifest, MergeResult, ReleaseNotesArchive, VersionIndex } from './types'

export const MAX_MERGE = 5

const ZH_SEPARATOR = '\n\n<!-- zh -->\n\n'

export type ArchiveFetcher = (version: string) => Promise<ReleaseNotesArchive | null>

/**
 * Coerce loose version strings ("v0.11.0-alpha.6", "0.11.0-alpha.6") to canonical
 * semver form so comparisons work.
 */
function normalizeVersion(version: string): string | null {
  const stripped = version.replace(/^v/, '')
  const coerced = semver.parse(stripped)
  return coerced ? coerced.version : null
}

function semverEq(a: string, b: string): boolean {
  const na = normalizeVersion(a)
  const nb = normalizeVersion(b)
  if (!na || !nb) return false
  return semver.eq(na, nb)
}

/**
 * Sort versions in descending semver order (newest first).
 * Stable — entries that compare equal keep relative order.
 */
export function sortVersionsDesc<T extends { version: string }>(versions: T[]): T[] {
  return [...versions].sort((a, b) => {
    const na = normalizeVersion(a.version) ?? '0.0.0'
    const nb = normalizeVersion(b.version) ?? '0.0.0'
    return semver.rcompare(na, nb)
  })
}

/**
 * Build the combined-notes markdown body. Layout:
 *
 *   > <prelude>
 *   ## v<latest>
 *   <notes_en>
 *   ## v<...>
 *   ...
 *
 *   <!-- zh -->
 *
 *   > <prelude_zh>
 *   ## v<latest>
 *   <notes_zh>
 *   ...
 */
export function buildCombinedNotes(
  archives: ReleaseNotesArchive[],
  options: { truncated: boolean; omittedCount: number; fromVersion: string }
): string {
  if (archives.length === 0) {
    return ''
  }

  const sorted = sortVersionsDesc(archives) as ReleaseNotesArchive[]
  const versionCount = sorted.length

  const preludeEn = buildPrelude('en', {
    versionCount,
    truncated: options.truncated,
    omittedCount: options.omittedCount,
    fromVersion: options.fromVersion,
  })
  const preludeZh = buildPrelude('zh', {
    versionCount,
    truncated: options.truncated,
    omittedCount: options.omittedCount,
    fromVersion: options.fromVersion,
  })

  const enBody = sorted
    .map(archive => `## v${archive.version}\n\n${archive.notes_en.trim()}`)
    .join('\n\n')

  const zhBody = sorted
    .map(archive => `## v${archive.version}\n\n${archive.notes_zh.trim()}`)
    .join('\n\n')

  const en = `${preludeEn}\n\n${enBody}`.trim()
  const zh = `${preludeZh}\n\n${zhBody}`.trim()

  return en + ZH_SEPARATOR + zh
}

function buildPrelude(
  lang: 'en' | 'zh',
  opts: { versionCount: number; truncated: boolean; omittedCount: number; fromVersion: string }
): string {
  const count = opts.versionCount
  if (lang === 'en') {
    const head =
      count === 1
        ? `> Cumulative changes since v${opts.fromVersion}.`
        : `> Cumulative changes across ${count} versions since v${opts.fromVersion} (newest first).`
    if (opts.truncated) {
      return `${head}\n> ${opts.omittedCount} older version(s) omitted — view full history at the changelog page.`
    }
    return head
  }
  const head =
    count === 1
      ? `> 自 v${opts.fromVersion} 起的累计变更。`
      : `> 自 v${opts.fromVersion} 起跨越 ${count} 个版本的累计变更（新版本在前）。`
  if (opts.truncated) {
    return `${head}\n> 另有 ${opts.omittedCount} 个更早版本已省略，完整历史请见 changelog 页面。`
  }
  return head
}

/**
 * Pure orchestration: given a latest manifest + channel index + a way to fetch
 * per-version archives, produce a manifest with merged notes.
 *
 * Edge cases (mirrors ADR §2.6):
 *   - fromVersion not in index → return latestManifest unchanged (mergedCount=1)
 *   - fromVersion === latest (fromIdx === 0) → return latestManifest unchanged
 *   - more than MAX_MERGE candidates → truncate, mark truncated=true
 *   - any archive fetch returns null → skip it (do not fail the request)
 */
export async function mergeNotes(
  latestManifest: Manifest,
  index: VersionIndex,
  fromVersion: string,
  fetchArchive: ArchiveFetcher
): Promise<MergeResult> {
  const fromIdx = index.versions.findIndex(v => semverEq(v.version, fromVersion))

  // Edge case A: from unknown — fall back to latest-only notes (do not break update flow).
  if (fromIdx === -1) {
    return passthrough(latestManifest)
  }

  // Edge case B: from is already latest — nothing to merge.
  if (fromIdx === 0) {
    return passthrough(latestManifest)
  }

  // index is sorted descending, so (from, latest] = index[0..fromIdx)
  const candidates = index.versions.slice(0, fromIdx)
  const selected = candidates.slice(0, MAX_MERGE)
  const truncated = candidates.length > MAX_MERGE
  const omittedCount = Math.max(0, candidates.length - MAX_MERGE)

  const archives = (await Promise.all(selected.map(v => fetchArchive(v.version)))).filter(
    (a): a is ReleaseNotesArchive => a !== null
  )

  if (archives.length === 0) {
    // None of the archives could be loaded — degrade rather than break.
    return passthrough(latestManifest)
  }

  const mergedNotes = buildCombinedNotes(archives, {
    truncated,
    omittedCount,
    fromVersion,
  })

  return {
    manifest: { ...latestManifest, notes: mergedNotes },
    truncated,
    mergedCount: archives.length,
    omittedCount,
  }
}

function passthrough(latest: Manifest): MergeResult {
  return { manifest: latest, truncated: false, mergedCount: 1, omittedCount: 0 }
}
