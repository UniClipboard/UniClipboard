import { convertFileSrc } from '@tauri-apps/api/core'

const UC_PROTOCOL_RE = /^uc:\/\/(.+)$/

/**
 * Resolve a uc:// custom protocol URL to a platform-correct format.
 *
 * Tauri 2 custom URI scheme protocols use different URL formats per platform:
 *   - macOS/Linux: uc://localhost/{path}
 *   - Windows: http://uc.localhost/{path}
 *
 * Non-uc:// URLs are returned unchanged.
 */
export function resolveUcUrl(ucUrl: string): string {
  const match = UC_PROTOCOL_RE.exec(ucUrl)
  if (!match) {
    return ucUrl
  }
  return convertFileSrc(match[1], 'uc')
}
