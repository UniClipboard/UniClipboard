import { useSyncExternalStore } from 'react'

/**
 * Window-width tiers of the macOS three-column window (sidebar | list | detail).
 * The design anchors at 1280px: 220 + 560 + 500.
 *
 * - `wide` (>= 1440): full sidebar; the list may grow to 720px.
 * - `standard` (1100-1439): full sidebar; the list keeps 560px until the detail
 *   reaches its floor, then gives way.
 * - `compact` (< 1100, window minimum 900): the sidebar is hidden; its toggle
 *   opens it as an overlay drawer.
 */
export type HistoryLayoutTier = 'compact' | 'standard' | 'wide'

export const STANDARD_MIN_WIDTH = 1100
export const WIDE_MIN_WIDTH = 1440

export function historyLayoutTier(windowWidth: number): HistoryLayoutTier {
  if (windowWidth >= WIDE_MIN_WIDTH) return 'wide'
  if (windowWidth >= STANDARD_MIN_WIDTH) return 'standard'
  return 'compact'
}

/** List-column constraints per tier, in px. Pixels, because
 * react-resizable-panels resolves `rem` against the body font size. */
export const LIST_COLUMN: Record<HistoryLayoutTier, { default: number; min: number; max: number }> =
  {
    compact: { default: 400, min: 360, max: 520 },
    standard: { default: 560, min: 360, max: 560 },
    wide: { default: 560, min: 400, max: 720 },
  }

/** The detail column never gets narrower than this; its header, copy facts and
 * footer need it. At the 900px window minimum the sidebar is hidden, and
 * 360 list + 420 detail still fits. */
export const DETAIL_COLUMN_MIN = 420

/** Clamp a remembered list width into a tier's range. */
export function clampListWidth(tier: HistoryLayoutTier, width: number): number {
  const { min, max } = LIST_COLUMN[tier]
  return Math.min(max, Math.max(min, Math.round(width)))
}

function subscribe(onChange: () => void): () => void {
  window.addEventListener('resize', onChange)
  return () => window.removeEventListener('resize', onChange)
}

const getTier = () => historyLayoutTier(window.innerWidth)

/** Current tier; re-renders only when the window crosses a tier boundary. */
export function useHistoryLayoutTier(): HistoryLayoutTier {
  return useSyncExternalStore(subscribe, getTier, getTier)
}

const LIST_WIDTH_STORAGE_KEY = 'uc.history.listWidth.v1'

/** The list width the user last dragged to in this tier, if any. A per-viewer
 * convenience: unreadable storage just means the tier default. */
export function readListWidth(tier: HistoryLayoutTier): number | null {
  try {
    const saved = JSON.parse(localStorage.getItem(LIST_WIDTH_STORAGE_KEY) ?? '{}') as Record<
      string,
      unknown
    >
    const width = saved[tier]
    return typeof width === 'number' && Number.isFinite(width) ? clampListWidth(tier, width) : null
  } catch {
    return null
  }
}

export function writeListWidth(tier: HistoryLayoutTier, width: number): void {
  try {
    const saved = JSON.parse(localStorage.getItem(LIST_WIDTH_STORAGE_KEY) ?? '{}') as Record<
      string,
      unknown
    >
    saved[tier] = clampListWidth(tier, width)
    localStorage.setItem(LIST_WIDTH_STORAGE_KEY, JSON.stringify(saved))
  } catch {
    // Storage unavailable: the width simply is not remembered.
  }
}

const LIBRARY_HIDDEN_STORAGE_KEY = 'uc.library.hidden.v1'

/** Whether the user hid the Library sidebar (standard and wide tiers). A
 * per-viewer convenience: unreadable storage means shown. */
export function readLibraryHidden(): boolean {
  try {
    return localStorage.getItem(LIBRARY_HIDDEN_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

export function writeLibraryHidden(hidden: boolean): void {
  try {
    localStorage.setItem(LIBRARY_HIDDEN_STORAGE_KEY, hidden ? '1' : '0')
  } catch {
    // Storage unavailable: the choice lasts for this session only.
  }
}
