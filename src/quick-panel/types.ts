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
  /**
   * paste_rep 已 `Lost` —— 点击粘贴会回 daemon 410。面板渲染时灰显并加
   * 删除线, 让用户在点击之前就识别。语义与 dashboard 列表一致。
   */
  isUnavailable: boolean
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
