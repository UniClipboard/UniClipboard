import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutScope } from '@/hooks/useShortcutScope'
import { QUICK_PANEL_SCALE_SHORTCUTS } from '@/shortcuts/definitions'
import {
  adjustQuickPanelScale,
  type QuickPanelScaleAction,
  type QuickPanelScaleFeedback,
} from '../window-layout'
import { useQuickPanelShortcutOverrides } from './useQuickPanelShortcutOverrides'

export function useQuickPanelScaleShortcuts(onAdjust: (feedback: QuickPanelScaleFeedback) => void) {
  const overrides = useQuickPanelShortcutOverrides()
  useShortcutScope('quickPanel')
  const binding = (index: number, action: QuickPanelScaleAction) => ({
    ...QUICK_PANEL_SCALE_SHORTCUTS[index],
    overrides,
    capture: true,
    enableOnFormTags: true,
    handler: () => onAdjust(adjustQuickPanelScale(action)),
  })
  useShortcut(binding(0, 'windowIncrease'))
  useShortcut(binding(1, 'windowDecrease'))
  useShortcut(binding(2, 'textIncrease'))
  useShortcut(binding(3, 'textDecrease'))
}
