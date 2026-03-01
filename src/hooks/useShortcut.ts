import { useHotkeys } from 'react-hotkeys-hook'
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
  enabled = true,
  handler,
  preventDefault = true,
}: UseShortcutOptions): void => {
  const { activeScope, activeLayer } = useShortcutContext()

  // global scope 在非 modal 层时始终激活，其他 scope 保持精确匹配
  const isActive =
    scope === 'global' ? activeLayer !== 'modal' && enabled : activeScope === scope && enabled

  useHotkeys(
    key,
    handler,
    {
      enabled: isActive,
      preventDefault,
      enableOnFormTags: false,
      enableOnContentEditable: false,
      // 使用非逗号字符作为多快捷键分隔符，避免 "mod+," 中的逗号被误判为分隔符
      delimiter: '§',
    },
    [key, scope, enabled, activeScope, activeLayer, handler, preventDefault]
  )
}
