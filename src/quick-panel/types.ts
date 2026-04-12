import type { ItemType } from '@/lib/clipboard-utils'

export type TimeRangePreset =
  | 'all_time'
  | 'today'
  | 'yesterday'
  | 'last_7d'
  | 'last_30d'
  | 'this_week'
  | 'this_month'

export interface DisplayItem {
  id: string
  type: ItemType
  preview: string
  activeTime: number
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
