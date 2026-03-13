export const isMac =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent)

/**
 * Format a modifier key for display using platform-appropriate symbols.
 * On macOS: mod/meta -> Cmd symbol, alt -> Option symbol, shift -> Shift symbol, ctrl -> Ctrl symbol
 * On other platforms: mod/meta -> Ctrl, alt -> Alt, shift -> Shift, ctrl -> Ctrl
 */
export function formatKeyPart(part: string): string {
  const lower = part.toLowerCase().trim()

  if (isMac) {
    switch (lower) {
      case 'mod':
      case 'meta':
      case 'cmd':
      case 'command':
        return '\u2318'
      case 'alt':
      case 'option':
        return '\u2325'
      case 'shift':
        return '\u21E7'
      case 'ctrl':
      case 'control':
        return '\u2303'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  } else {
    switch (lower) {
      case 'mod':
      case 'meta':
      case 'cmd':
      case 'command':
      case 'ctrl':
      case 'control':
        return 'Ctrl'
      case 'alt':
      case 'option':
        return 'Alt'
      case 'shift':
        return 'Shift'
      default:
        return part.charAt(0).toUpperCase() + part.slice(1)
    }
  }
}
