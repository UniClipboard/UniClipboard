import semver from 'semver'
import type { Manifest, MergeResult, ReleaseNotesArchive, VersionIndex } from './types'

export const MAX_MERGE = 5

const ZH_SEPARATOR = '\n\n<!-- zh -->\n\n'

export type ArchiveFetcher = (version: string) => Promise<ReleaseNotesArchive | null>

/**
 * 把宽松的版本字符串（如 "v0.11.0-alpha.6"、"0.11.0-alpha.6"）
 * 规范化为标准 semver 形式，使后续比较一致。
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
 * 按 semver 降序排序（最新版本在前）。
 * 稳定排序——比较相等的条目保留原相对顺序。
 */
export function sortVersionsDesc<T extends { version: string }>(versions: T[]): T[] {
  return versions.toSorted((a, b) => {
    const na = normalizeVersion(a.version) ?? '0.0.0'
    const nb = normalizeVersion(b.version) ?? '0.0.0'
    return semver.rcompare(na, nb)
  })
}

/**
 * 构造合并后的 notes markdown 内容。布局：
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
 * 纯编排逻辑：给定最新 manifest + channel 索引 + 单版本归档的获取函数，
 * 生成一份 notes 已合并的 manifest。
 *
 * 边界情况（对应 ADR §2.6）：
 *   - fromVersion 不在索引中 → 原样返回 latestManifest（mergedCount=1）
 *   - fromVersion === latest（fromIdx === 0）→ 原样返回 latestManifest
 *   - 候选超过 MAX_MERGE → 截断并标记 truncated=true
 *   - 某个版本归档拉取返回 null → 跳过它（不让整个请求失败）
 *   - 最新版本的归档缺失 → 用 latestManifest.notes 合成兜底，
 *     保证合并 notes 始终覆盖用户正在升级到的版本
 */
export async function mergeNotes(
  latestManifest: Manifest,
  index: VersionIndex,
  fromVersion: string,
  fetchArchive: ArchiveFetcher
): Promise<MergeResult> {
  const fromIdx = index.versions.findIndex(v => semverEq(v.version, fromVersion))

  // 边界 A：from 未知（老版本、dev build、未来版本）—— 回落到「只返回 latest」，
  // 不能因为 from 未识别而打断升级流程。
  if (fromIdx === -1) {
    return passthrough(latestManifest)
  }

  // 边界 B：from 已是最新版 —— 没有任何需要合并的内容。
  if (fromIdx === 0) {
    return passthrough(latestManifest)
  }

  // 索引为降序，所以区间 (from, latest] = index[0..fromIdx)
  const candidates = index.versions.slice(0, fromIdx)
  const selected = candidates.slice(0, MAX_MERGE)
  const truncated = candidates.length > MAX_MERGE
  const omittedCount = Math.max(0, candidates.length - MAX_MERGE)

  const archives = (await Promise.all(selected.map(v => fetchArchive(v.version)))).filter(
    (a): a is ReleaseNotesArchive => a !== null
  )

  // 兜底：若最新版本的归档加载失败（R2 状态不一致、发布部分上传、人工删除等），
  // 用 latestManifest 合成一份，保证用户「升级到」的目标版本的描述永远不会被
  // 静默丢失。selected[0] 一定是最新版（索引为降序），所以这条分支只在
  // fetchArchive(latest) 返回 null 时触发。
  const latestSelected = selected[0]
  const hasLatestArchive =
    latestSelected !== undefined && archives.some(a => semverEq(a.version, latestSelected.version))
  if (latestSelected && !hasLatestArchive) {
    const { notes_en, notes_zh } = splitLegacyMergedNotes(latestManifest.notes)
    archives.unshift({
      version: latestManifest.version,
      channel: index.channel,
      pub_date: latestManifest.pub_date,
      notes_en,
      notes_zh,
    })
  }

  if (archives.length === 0) {
    // 全部归档都拉不到 —— 降级而非报错。
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

/**
 * 把旧格式 `<en>\n\n<!-- zh -->\n\n<zh>`（由
 * scripts/assemble-update-manifest.js 写出）拆回两种语言。
 * 若没有 `<!-- zh -->` 分隔符，则整段当作 `notes_en` 返回。
 */
function splitLegacyMergedNotes(notes: string): { notes_en: string; notes_zh: string } {
  const idx = notes.indexOf(ZH_SEPARATOR)
  if (idx === -1) {
    return { notes_en: notes, notes_zh: '' }
  }
  return {
    notes_en: notes.slice(0, idx),
    notes_zh: notes.slice(idx + ZH_SEPARATOR.length),
  }
}

function passthrough(latest: Manifest): MergeResult {
  return { manifest: latest, truncated: false, mergedCount: 1, omittedCount: 0 }
}
