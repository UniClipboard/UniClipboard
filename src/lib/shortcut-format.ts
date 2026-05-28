export const isMac =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)

/**
 * Format a modifier key for display using platform-appropriate symbols.
 *
 * Physical key mapping:
 *   meta/super = the OS "super" key (Cmd on macOS ⌘, Win on Windows)
 *   ctrl       = the physical Control key (⌃ on macOS, Ctrl on Windows)
 *   mod/cmd    = abstract "platform modifier" (= meta on macOS, ctrl on Windows)
 */
function formatKeyPart(part: string): string {
  const lower = part.toLowerCase().trim()

  if (isMac) {
    switch (lower) {
      // Physical Meta key (Cmd) and abstract platform modifier (mod/cmd) are
      // the same key on macOS → ⌘
      case 'mod':
      case 'meta':
      case 'cmd':
      case 'command':
      case 'super':
        return '\u2318'
      case 'alt':
      case 'option':
        return '\u2325'
      case 'shift':
        return '\u21E7'
      case 'ctrl':
      case 'control':
        return '\u2303'
      case 'minus':
      case 'subtract':
        return '-'
      case 'equal':
      case 'add':
        return '+'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  } else {
    switch (lower) {
      // Physical Meta key (Win key) — distinct from Ctrl on Windows
      case 'meta':
      case 'super':
        return 'Win'
      // Abstract platform modifier (mod/cmd) maps to Ctrl on Windows
      case 'mod':
      case 'cmd':
      case 'command':
        return 'Ctrl'
      // Physical Control key
      case 'ctrl':
      case 'control':
        return 'Ctrl'
      case 'alt':
      case 'option':
        return 'Alt'
      case 'shift':
        return 'Shift'
      case 'minus':
      case 'subtract':
        return '-'
      case 'equal':
      case 'add':
        return '+'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  }
}

export function formatShortcutParts(shortcut: string): string[] {
  const rawParts = shortcut.split('+').flatMap(part => {
    const trimmed = part.trim()
    return trimmed ? [trimmed] : []
  })
  const lowerParts = rawParts.map(part => part.toLowerCase())

  if (lowerParts.includes('shift') && lowerParts.includes('equal')) {
    const filtered = rawParts.filter(part => part.toLowerCase() !== 'shift')
    return filtered.map(part => (part.toLowerCase() === 'equal' ? '+' : formatKeyPart(part)))
  }

  return rawParts.map(formatKeyPart)
}
