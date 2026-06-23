import { use, useCallback, useRef } from 'react'
import { useHotkeys } from 'react-hotkeys-hook'
import { SettingContext } from '@/contexts/setting-context'
import { useShortcutContext } from '@/contexts/shortcut-context'
import { ShortcutScope } from '@/shortcuts/definitions'
import { splitChord } from '@/shortcuts/normalize'

/**
 * Window (ms) within which the second chord segment must follow the leader.
 * Mirrors the Rust-side OS chord window so single-vs-chord feel is consistent.
 */
const CHORD_WINDOW_MS = 1000

/**
 * useShortcut Hook 选项
 */
interface UseShortcutOptions {
  /** 快捷键组合，如 "esc", "cmd+a", "mod+comma"；两段 chord 用空格分隔，支持字符串或数组形式 */
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
  /** 是否允许在表单元素中触发 */
  enableOnFormTags?: boolean | Array<'input' | 'textarea' | 'select'>
}

/**
 * 快捷键注册 Hook
 *
 * 基于 react-hotkeys-hook 封装，支持作用域隔离、条件启用，以及 VS Code 风格的
 * 两段 chord（值里用空格分隔；第一段是 leader）。react-hotkeys-hook v5 不原生
 * 支持按键序列，所以 chord 由本 hook 用一个轻量状态机协调：按下 leader 进入
 * pending，在 {@link CHORD_WINDOW_MS} 内按下第二段才触发。两段相同（即“双击”
 * 同一组合）作为时间窗特例处理。
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
  enableOnFormTags = false,
}: UseShortcutOptions): void => {
  const { activeScope, activeLayer } = useShortcutContext()

  // Get setting context for keyboard shortcuts override support
  // This is optional - only used when id is provided
  const settingContext = use(SettingContext)
  const keyboardShortcuts = settingContext?.setting?.keyboardShortcuts ?? null

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

  // Chord support: a single-string binding may be a two-segment sequence
  // ("A B"). Array bindings (alternatives) are never chorded.
  const sequence = typeof effectiveKey === 'string' ? splitChord(effectiveKey) : []
  const isChord = sequence.length >= 2
  const leaderKey = sequence[0] ?? ''
  const secondKey = sequence[1] ?? ''
  const sameSecond = isChord && secondKey === leaderKey

  // Primary hotkey: the leader segment when chorded, otherwise the raw
  // effectiveKey (which may carry array alternatives that react-hotkeys-hook
  // handles natively).
  const primaryHotkey = isChord ? leaderKey : effectiveKey

  // Chord progress is tracked in refs so it survives re-renders without
  // re-registering the hotkeys.
  const pendingRef = useRef(false)
  const lastLeaderAtRef = useRef(0)

  const onPrimary = useCallback(() => {
    if (!isChord) {
      handler()
      return
    }
    const now = Date.now()
    if (sameSecond) {
      // Double tap of the same combo: second press within the window fires.
      if (pendingRef.current && now - lastLeaderAtRef.current <= CHORD_WINDOW_MS) {
        pendingRef.current = false
        handler()
      } else {
        pendingRef.current = true
        lastLeaderAtRef.current = now
      }
    } else {
      // Leader of a two-distinct-combo chord: arm and wait for the second key.
      pendingRef.current = true
      lastLeaderAtRef.current = now
    }
  }, [isChord, sameSecond, handler])

  const onSecond = useCallback(() => {
    // Only the distinct-second-segment chord uses this; the same-combo double
    // tap is fully handled in onPrimary.
    if (pendingRef.current && Date.now() - lastLeaderAtRef.current <= CHORD_WINDOW_MS) {
      pendingRef.current = false
      handler()
    }
  }, [handler])

  useHotkeys(
    primaryHotkey,
    onPrimary,
    {
      enabled: isActive,
      preventDefault,
      enableOnFormTags,
      enableOnContentEditable: false,
      // 使用非逗号字符作为多快捷键分隔符，避免 "mod+," 中的逗号被误判为分隔符
      delimiter: '§',
    },
    [
      primaryHotkey,
      scope,
      enabled,
      activeScope,
      activeLayer,
      onPrimary,
      preventDefault,
      enableOnFormTags,
      keyboardShortcuts,
    ]
  )

  // Second-segment listener: only armed for a chord whose two segments differ.
  // `f13` is an inert placeholder when there's no distinct second segment.
  useHotkeys(
    secondKey || 'f13',
    onSecond,
    {
      enabled: isActive && isChord && !sameSecond,
      preventDefault,
      enableOnFormTags,
      enableOnContentEditable: false,
      delimiter: '§',
    },
    [secondKey, isChord, sameSecond, isActive, onSecond, preventDefault, enableOnFormTags]
  )
}
