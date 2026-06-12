/**
 * UI domain model for clipboard entries.
 *
 * `ClipboardEntry` is the single frontend shape for a clipboard history
 * entry: it is produced from the daemon's `EntryProjectionDto` by
 * `projectClipboardEntry` (see `clipboard-transform.ts`), held as-is in
 * stores (`clipboardSlice`, `useClipboardCollection`), and rendered by
 * components. Daemon DTO field changes should only ever touch the
 * projection, never the consumers.
 */

export interface ClipboardTextItem {
  display_text: string
  /** Whether full content is available for detail fetch (preview is truncated). */
  has_detail: boolean
  size: number
}

export interface ClipboardImageItem {
  thumbnail?: string | null
  size: number
  width: number
  height: number
}

export interface ClipboardFileItem {
  file_names: string[]
  file_sizes: number[]
  /**
   * Per-file missing flag, aligned with `file_names` / `file_sizes` by index.
   * `true` means the file never finished materializing when the entry was
   * persisted (the user cancelled the inbound transfer): it cannot be
   * opened/copied/dragged, but the entry itself survives (deletable, keeps
   * filename/size). Absent means all false (backward compatible with
   * historical entries and non-file entries).
   */
  file_missing?: boolean[]
}

export interface ClipboardLinkItem {
  urls: string[]
  domains: string[]
}

export interface ClipboardCodeItem {
  code: string
}

export type ClipboardEntryType = 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'

export type ClipboardEntryContent =
  | ClipboardTextItem
  | ClipboardImageItem
  | ClipboardFileItem
  | ClipboardLinkItem
  | ClipboardCodeItem

export interface ClipboardEntry {
  id: string
  /** Display type; discriminates how `content` should be interpreted. */
  type: ClipboardEntryType
  content: ClipboardEntryContent | null
  /** Capture timestamp (epoch ms). */
  createdAt: number
  updatedAt: number
  /** Timestamp (epoch ms) used for ordering and date grouping. */
  activeTime: number
  isFavorited: boolean
  /**
   * True when the paste representation's payload is `Lost` — restoring would
   * get a daemon 410. Rows grey the entry out and badge it so the user can
   * tell before clicking (see `DaemonErrorCode.PAYLOAD_UNAVAILABLE`).
   */
  isUnavailable: boolean
}

/**
 * View model rendered by clipboard rows and the preview pane.
 *
 * Browse rows are a `ClipboardEntry` plus a formatted relative-time string
 * (locale- and tick-dependent, so it stays a render-time concern). Search
 * results and pending inbound placeholders synthesize partial items, hence
 * the optional fields.
 */
export interface DisplayClipboardItem {
  id: string
  type: ClipboardEntryType
  content: ClipboardEntryContent | null
  /** Formatted relative-time label (e.g. "5 minutes ago"). */
  time: string
  activeTime: number
  isFavorited?: boolean
  isUnavailable?: boolean
  /** Source device name, only for pending inbound placeholder rows. */
  device?: string
  /** Fallback preview text when `content` is unavailable (search/pending rows). */
  textPreview?: string
}
