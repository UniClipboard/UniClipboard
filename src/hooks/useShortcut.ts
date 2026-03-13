import { useContext } from 'react'
import { useHotkeys } from 'react-hotkeys-hook'
import { SettingContext } from '@/contexts/setting-context'
import { useShortcutContext } from '@/contexts/shortcut-context'
import { ShortcutScope } from '@/shortcuts/definitions'

/**
 * useShortcut Hook 选项
 */
interface UseShortcutOptions {
  /** 快捷键组合，如 "esc", "cmd+a", "mod+comma"，支持字符串或数组形式 */
  key: string | string[]
  /** 作用域 */
  scope: ShortcutScope
  /** 快捷键定义ID（可选，用于从设置中读取覆盖的键位） */
  id?: string
  /** 是否启用（可选，默认 true） */
  enabled?: boolean
  /** 触发时的处理函数 */
  handler: () => void
  /** 是否阻止默认行为（可选，默认 true） */
  preventDefault?: boolean
}

/**
 * 快捷键注册 Hook
 *
 * 基于 react-hotkeys-hook 封装，支持作用域隔离和条件启用
 *
 * @example
 * ```tsx
 * useShortcut({
 *   key: "esc",
 *   scope: "clipboard",
 *   enabled: selectedIds.size > 0,
 *   handler: () => setSelectedIds(new Set()),
 * });
 * ```
 */
export const useShortcut = ({
  key,
  scope,
  id,
  enabled = true,
  handler,
  preventDefault = true,
}: UseShortcutOptions): void => {
  const { activeScope, activeLayer } = useShortcutContext()

  // Get setting context for keyboard shortcuts override support
  // This is optional - only used when id is provided
  const settingContext = useContext(SettingContext)
  const keyboardShortcuts = settingContext?.setting?.keyboard_shortcuts ?? null

  // Determine effective key: use override from settings if available
  const effectiveKey = (() => {
    if (!id || !keyboardShortcuts) {
      return key
    }
    // Check if there's an override for this id
    const override = keyboardShortcuts[id]
    if (override != null) {
      return Array.isArray(override) ? (override[0] ?? key) : override
    }
    return key
  })()

  // global scope 在非 modal 层时始终激活，其他 scope 保持精确匹配
  const isActive =
    scope === 'global' ? activeLayer !== 'modal' && enabled : activeScope === scope && enabled

  useHotkeys(
    effectiveKey,
    handler,
    {
      enabled: isActive,
      preventDefault,
      enableOnFormTags: false,
      enableOnContentEditable: false,
      // 使用非逗号字符作为多快捷键分隔符，避免 "mod+," 中的逗号被误判为分隔符
      delimiter: '§',
    },
    [
      effectiveKey,
      scope,
      enabled,
      activeScope,
      activeLayer,
      handler,
      preventDefault,
      keyboardShortcuts,
    ]
  )
}
