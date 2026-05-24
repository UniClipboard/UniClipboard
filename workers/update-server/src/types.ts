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
