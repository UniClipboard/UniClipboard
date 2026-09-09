import { commands } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import { detectPlatformInfo } from '@/lib/platform'
import { adjustUiScale, readStoredUiScale } from '@/lib/ui-scale'

const STORAGE_KEY = 'uniclipboard.quickPanel.windowScale'
const MIN_WINDOW_SCALE = 0.8
const MAX_WINDOW_SCALE = 1.5
const WINDOW_SCALE_STEP = 0.1
const log = createLogger('quick-panel-window-layout')
let sessionScale: number | undefined

function normalizeScale(value: number): number {
  return Number.isFinite(value)
    ? Math.round(Math.min(MAX_WINDOW_SCALE, Math.max(MIN_WINDOW_SCALE, value)) * 10) / 10
    : 1
}

function readWindowScale(): number {
  if (sessionScale !== undefined) return sessionScale
  try {
    const value = localStorage.getItem(STORAGE_KEY)
    return value === null || value.trim() === '' ? 1 : normalizeScale(Number(value))
  } catch {
    return 1
  }
}

function isLinuxPanel(): boolean {
  const { isLinux, isTauri } = detectPlatformInfo()
  return isLinux && isTauri
}

export async function setQuickPanelLayout(scale: number, previewExpanded: boolean): Promise<void> {
  const factor = isLinuxPanel() ? readWindowScale() : 1
  await commands.setQuickPanelLayout(scale, previewExpanded, factor)
}

export type QuickPanelScaleFeedback = { textPercent: number; windowPercent: number }
export type QuickPanelScaleAction =
  | 'windowIncrease'
  | 'windowDecrease'
  | 'textIncrease'
  | 'textDecrease'

export function adjustQuickPanelScale(action: QuickPanelScaleAction): QuickPanelScaleFeedback {
  const direction = action.endsWith('Increase') ? 1 : -1
  let textScale = readStoredUiScale()
  if (action.startsWith('text')) {
    textScale = adjustUiScale(direction > 0 ? 'in' : 'out')
  } else {
    const previous = readWindowScale()
    const next = normalizeScale(previous + direction * WINDOW_SCALE_STEP)
    if (next !== previous) {
      sessionScale = next
      try {
        localStorage.setItem(STORAGE_KEY, String(next))
      } catch (err) {
        log.warn({ err }, 'failed to persist quick panel window size')
      }
      void setQuickPanelLayout(textScale, false).catch(err => {
        log.warn({ err }, 'failed to resize quick panel window')
      })
    }
  }
  return {
    textPercent: Math.round(textScale * 100),
    windowPercent: Math.round(readWindowScale() * 100),
  }
}
