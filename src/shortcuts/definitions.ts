/**
 * Shortcut action types
 * Union type of all shortcut actions
 */
import { isMac } from '@/lib/shortcut-format'
import { ShortcutLayer } from './layers'

export type ShortcutAction =
  | 'global.zoomIn'
  | 'global.zoomOut'
  | 'global.toggleQuickPanel'
  | 'navigation.settings'
  | string

/**
 * Shortcut scope
 * Used to isolate shortcuts across different pages/components
 */
export type ShortcutScope = 'global' | 'clipboard' | 'settings' | 'devices' | 'modal'

/**
 * Default scope -> layer mapping
 *
 * - global: Global layer (always active)
 * - page: Page layer (e.g. clipboard/settings/devices)
 * - modal: Modal layer (when a modal is open)
 */
export const DEFAULT_SCOPE_LAYER: Record<ShortcutScope, ShortcutLayer> = {
  global: 'global',
  clipboard: 'page',
  settings: 'page',
  devices: 'page',
  modal: 'modal',
}

/**
 * Shortcut definition interface
 */
export interface ShortcutDefinition {
  /** Unique identifier */
  id: string
  /** Key combination, e.g. "esc", "cmd+a", "mod+comma"; string or array */
  key: string | string[]
  /** Action type */
  action: ShortcutAction
  /** Scope */
  scope: ShortcutScope
  /** i18n key for the description text */
  description: string
  /** Whether to prevent default browser behavior */
  preventDefault?: boolean
}

/**
 * Central shortcut definitions
 *
 * 仅收录"在前端/后端真正被实装"的快捷键。
 * 历史上这里还有 clipboard.{esc,selectAll,delete,favorite}、nav.{dashboard,devices}、
 * search.focus、modal.close —— 它们只在设置页面"看起来可定制",但没有 handler 读取
 * override,改了也不生效,因此从设置面板里下掉,避免对用户产生误导。
 */
export const SHORTCUT_DEFINITIONS: ShortcutDefinition[] = [
  // ===== Navigation =====
  {
    id: 'nav.settings',
    key: 'mod+comma',
    action: 'navigation.settings',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.goSettings',
  },

  // ===== Global (OS-level) =====
  {
    id: 'global.toggleQuickPanel',
    key: isMac ? 'meta+ctrl+v' : 'ctrl+alt+v',
    action: 'global.toggleQuickPanel',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.toggleQuickPanel',
  },
  {
    id: 'global.zoomIn',
    key: ['mod+shift+equal', 'mod+equal', 'mod+add'],
    action: 'global.zoomIn',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.zoomIn',
  },
  {
    id: 'global.zoomOut',
    key: ['mod+minus', 'mod+subtract'],
    action: 'global.zoomOut',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.zoomOut',
  },
]
