/**
 * Shortcut action types
 * Union type of all shortcut actions
 */
import { ShortcutLayer } from './layers'

export type ShortcutAction =
  | 'clipboard.clearSelection'
  | 'clipboard.selectAll'
  | 'clipboard.delete'
  | 'clipboard.favorite'
  | 'clipboard.copy'
  | 'navigation.dashboard'
  | 'navigation.devices'
  | 'navigation.settings'
  | 'search.focus'
  | 'modal.close'
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
 */
export const SHORTCUT_DEFINITIONS: ShortcutDefinition[] = [
  // ===== Clipboard actions =====
  {
    id: 'clipboard.esc',
    key: 'esc',
    action: 'clipboard.clearSelection',
    scope: 'clipboard',
    description: 'settings.sections.shortcuts.actions.clearSelection',
  },
  {
    id: 'clipboard.selectAll',
    key: 'mod+a',
    action: 'clipboard.selectAll',
    scope: 'clipboard',
    description: 'settings.sections.shortcuts.actions.selectAll',
  },
  {
    id: 'clipboard.delete',
    key: 'backspace',
    action: 'clipboard.delete',
    scope: 'clipboard',
    description: 'settings.sections.shortcuts.actions.delete',
  },
  {
    id: 'clipboard.favorite',
    key: 'mod+f',
    action: 'clipboard.favorite',
    scope: 'clipboard',
    description: 'settings.sections.shortcuts.actions.favorite',
  },

  // ===== Navigation =====
  {
    id: 'nav.dashboard',
    key: 'mod+1',
    action: 'navigation.dashboard',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.goClipboard',
  },
  {
    id: 'nav.devices',
    key: 'mod+2',
    action: 'navigation.devices',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.goDevices',
  },
  {
    id: 'nav.settings',
    key: 'mod+comma',
    action: 'navigation.settings',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.goSettings',
  },

  // ===== Search =====
  {
    id: 'search.focus',
    key: 'mod+/',
    action: 'search.focus',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.focusSearch',
  },

  // ===== Global (OS-level) =====
  {
    id: 'global.toggleQuickPanel',
    key: 'mod+ctrl+v',
    action: 'global.toggleQuickPanel',
    scope: 'global',
    description: 'settings.sections.shortcuts.actions.toggleQuickPanel',
  },

  // ===== Modal =====
  {
    id: 'modal.close',
    key: 'esc',
    action: 'modal.close',
    scope: 'modal',
    description: 'settings.sections.shortcuts.actions.closeModal',
  },
]
