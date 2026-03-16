import { isMac } from '@/lib/shortcut-format'

/**
 * Canonical modifier names used after normalization.
 *
 * We distinguish two "command-like" modifiers:
 *   - `meta`  – the physical Meta key (Cmd on macOS, Win on Windows).
 *               This is what the browser's KeyboardEvent.key reports as "Meta".
 *   - `ctrl`  – the physical Control key on every platform.
 *
 * The abstract token `mod` (from react-hotkeys-hook) is resolved to the
 * *actual* physical key for the current platform so that conflict detection
 * compares apples to apples:
 *   - macOS:  `mod` / `cmd` / `command` → `meta`
 *   - others: `mod` / `cmd` / `command` → `ctrl`
 */

const PLATFORM_MODIFIER_ALIASES: Record<string, string> = isMac
  ? {
      command: 'meta',
      cmd: 'meta',
      mod: 'meta',
      super: 'meta',
      control: 'ctrl',
      option: 'alt',
      escape: 'esc',
    }
  : {
      command: 'ctrl',
      cmd: 'ctrl',
      mod: 'ctrl',
      super: 'meta',
      meta: 'meta',
      control: 'ctrl',
      option: 'alt',
      escape: 'esc',
    }

const MODIFIER_ORDER = ['ctrl', 'alt', 'shift', 'meta'] as const

/**
 * 规范化快捷键字符串，便于冲突检测与比较。
 *
 * 目标格式示例：
 * - "meta+shift+k"   (Cmd+Shift+K on macOS, Win+Shift+K on Windows)
 * - "ctrl+v"         (Ctrl+V on all platforms)
 * - "ctrl+meta+v"    (Ctrl+Cmd+V on macOS, Ctrl+Win+V on Windows)
 * - "esc"
 */
export const normalizeHotkey = (key: string | string[]): string => {
  const normalizeSingleHotkey = (raw: string): string => {
    const tokens = raw
      .split('+')
      .map(t => t.trim().toLowerCase())
      .filter(Boolean)
      .map(t => PLATFORM_MODIFIER_ALIASES[t] ?? t)

    const modifiers = new Set<string>()
    const nonModifiers: string[] = []

    for (const token of tokens) {
      if ((MODIFIER_ORDER as readonly string[]).includes(token)) {
        modifiers.add(token)
        continue
      }
      nonModifiers.push(token)
    }

    const orderedModifiers = MODIFIER_ORDER.filter(m => modifiers.has(m))
    const base = nonModifiers.join('+')

    return base ? [...orderedModifiers, base].join('+') : orderedModifiers.join('+')
  }

  if (Array.isArray(key)) {
    return key
      .map(raw => normalizeSingleHotkey(raw ?? ''))
      .filter(Boolean)
      .join(',')
  }

  return normalizeSingleHotkey(key)
}
