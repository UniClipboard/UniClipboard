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
      '=': 'equal',
      '-': 'minus',
      add: 'add',
      subtract: 'subtract',
      command: 'meta',
      cmd: 'meta',
      mod: 'meta',
      super: 'meta',
      control: 'ctrl',
      option: 'alt',
      escape: 'esc',
    }
  : {
      '=': 'equal',
      '-': 'minus',
      add: 'add',
      subtract: 'subtract',
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

/** Max number of chord segments in a single binding (VS Code-style, leader + key). */
export const MAX_CHORD_SEGMENTS = 2

/**
 * Normalize a single key combo (no chord spaces): canonical modifier order +
 * platform aliasing. e.g. `"shift+ctrl+k"` → `"ctrl+shift+k"`.
 */
const normalizeSingleHotkey = (raw: string): string => {
  const tokens = raw.split('+').flatMap(t => {
    const trimmed = t.trim().toLowerCase()
    if (!trimmed) return []
    return [PLATFORM_MODIFIER_ALIASES[trimmed] ?? trimmed]
  })

  const modifiers = new Set<string>()
  const nonModifiers: string[] = []

  const modifierSet = new Set<string>(MODIFIER_ORDER)
  for (const token of tokens) {
    if (modifierSet.has(token)) {
      modifiers.add(token)
      continue
    }
    nonModifiers.push(token)
  }

  const orderedModifiers = MODIFIER_ORDER.filter(m => modifiers.has(m))
  const base = nonModifiers.join('+')

  return base ? [...orderedModifiers, base].join('+') : orderedModifiers.join('+')
}

/**
 * Split a chord sequence into its segments. One or two key combos separated by
 * a single space (VS Code style, e.g. `"meta+ctrl+v meta+ctrl+v"`). A single
 * combo (no space) yields one segment.
 */
export const splitChord = (sequence: string): string[] =>
  sequence
    .split(' ')
    .map(seg => seg.trim())
    .filter(Boolean)

/**
 * Normalize a whole chord sequence: each space-separated segment is normalized
 * independently and re-joined with single spaces. A single combo round-trips
 * to the same value as the legacy single-combo normalization. Capped at
 * {@link MAX_CHORD_SEGMENTS} segments (recording never produces more).
 */
export const normalizeChord = (sequence: string): string =>
  splitChord(sequence).slice(0, MAX_CHORD_SEGMENTS).map(normalizeSingleHotkey).join(' ')

/**
 * 规范化快捷键字符串，便于冲突检测与比较。
 *
 * 每个 binding 可以是单组合或两段 chord（空格分隔）；数组表示多个互为
 * 备选（alternatives）的 binding，归一化后用逗号连接。
 *
 * 目标格式示例：
 * - "meta+shift+k"            (Cmd+Shift+K on macOS, Win+Shift+K on Windows)
 * - "ctrl+v"                  (Ctrl+V on all platforms)
 * - "meta+ctrl+v meta+ctrl+v" (chord: 连按两次 Cmd+Ctrl+V)
 * - "esc"
 */
export const normalizeHotkey = (key: string | string[]): string => {
  if (Array.isArray(key)) {
    return key
      .flatMap(raw => {
        const normalized = normalizeChord(raw ?? '')
        return normalized ? [normalized] : []
      })
      .join(',')
  }

  return normalizeChord(key)
}
