export interface Manifest {
  version: string
  notes: string
  pub_date: string
  platforms: Record<string, { signature: string; url: string }>
}

export interface ReleaseNotesArchive {
  version: string
  channel: string
  pub_date: string
  notes_en: string
  notes_zh: string
}

export interface VersionIndex {
  channel: string
  updated_at: string
  versions: Array<{ version: string; pub_date: string }>
}

export interface MergeResult {
  manifest: Manifest
  truncated: boolean
  mergedCount: number
  omittedCount: number
}

/**
 * Android companion-app update manifest.
 *
 * Distinct from the desktop `Manifest`: the mobile app is not a Tauri updater,
 * so it carries per-ABI APK descriptors (name + sha256) instead of signed
 * platform tuples, and embeds bilingual notes inline rather than merging them
 * from a separate archive. Served verbatim from R2 at `android/{channel}.json`.
 */
export interface AndroidManifest {
  version: string
  tagName: string
  prerelease: boolean
  pub_date: string
  notes: { en: string; zh: string }
  assets: Array<{ name: string; sha256: string }>
}
