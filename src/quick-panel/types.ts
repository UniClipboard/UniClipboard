import type { ClipboardEntryType } from '@/lib/clipboard-entry'

// TimeRangePreset lives with the search API (it mirrors the backend `timePreset`
// param); re-exported here so existing quick-panel imports keep resolving.
export type { TimeRangePreset } from '@/api/daemon/search'

export interface DisplayItem {
  id: string
  type: ClipboardEntryType
  preview: string
  activeTime: number
  /**
   * paste_rep 已 `Lost` —— 点击粘贴会回 daemon 410。面板渲染时灰显并加
   * 删除线, 让用户在点击之前就识别。语义与 dashboard 列表一致。
   */
  isUnavailable: boolean
}

/**
 * Id-based handlers the quick panel injects into the reused history right-click
 * menu ({@link HistoryCardContextMenu}). Copy / favorite / delete are wired to
 * the panel's own data layer; send-to-device and reveal-in-folder are
 * self-contained inside the menu component.
 */
export interface QuickPanelContextMenuActions {
  /** Copy the entry to the system clipboard (the panel then dismisses). */
  onCopy: (id: string) => void
  onPasteFilePaths: (id: string) => void
  /** Toggle the entry's favorite flag; `current` is its present state. */
  onToggleFavorite: (id: string, current: boolean) => void
  /** Delete the entry immediately (no confirm dialog, matching Alt+Backspace). */
  onDelete: (id: string) => void
}

export type PreviewMode = 'closed' | 'reserving' | 'expanded'
export type PreviewFocusSource = 'selection' | 'hover'

export interface PreviewState {
  entryId: string | null
  mode: PreviewMode
  suppressed: boolean
  historyLockedWidth: number | null
  focusSource: PreviewFocusSource
}

export type PreviewAction =
  | { type: 'reset'; suppressed?: boolean }
  | { type: 'suppress'; value: boolean }
  | { type: 'set-entry'; entryId: string | null }
  | { type: 'set-focus-source'; source: PreviewFocusSource }
  | { type: 'reserve-space'; entryId: string; historyLockedWidth: number | null }
  | { type: 'expand' }
