import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { ShortcutScope } from '@/shortcuts/definitions'

/**
 * 快捷键作用域管理 Hook
 *
 * 用于组件设置当前激活的快捷键作用域。
 * 组件卸载时会自动恢复为之前的作用域（基于 layer + priority 的上下文栈计算）。
 *
 * @example
 * ```tsx
 * const DashboardPage: React.FC = () => {
 *   // 设置当前页面作用域为 clipboard
 *   useShortcutScope("clipboard");
 *
 *   return <div>...</div>;
 * };
 * ```
 *
 * @param scope - 要设置的作用域
 */
export const useShortcutScope = (scope: ShortcutScope, priority?: number): void => {
  useShortcutLayer({ layer: 'page', scope, priority })
}
