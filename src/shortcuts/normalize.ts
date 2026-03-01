const MODIFIER_ALIASES: Record<string, string> = {
  command: 'cmd',
  meta: 'cmd',
  mod: 'cmd',
  control: 'ctrl',
  option: 'alt',
  escape: 'esc',
}

const MODIFIER_ORDER = ['ctrl', 'alt', 'shift', 'cmd'] as const

/**
 * 规范化快捷键字符串，便于冲突检测与比较。
 *
 * 目标格式示例：
 * - "cmd+shift+k"
 * - "esc"
 * - "ctrl+alt+/"
 */
export const normalizeHotkey = (key: string | string[]): string => {
  const normalizeSingleHotkey = (raw: string): string => {
    const tokens = raw
      .split('+')
      .map(t => t.trim().toLowerCase())
      .filter(Boolean)
      .map(t => MODIFIER_ALIASES[t] ?? t)

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
