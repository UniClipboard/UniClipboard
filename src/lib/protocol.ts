import { isWindowsPlatform } from './utils'

const UC_PROTOCOL_RE = /^uc:\/\/(.+)$/

/**
 * Resolve a uc:// custom protocol URL to a platform-correct format.
 *
 * Tauri 2 custom URI scheme protocols use different URL formats per platform:
 *   - macOS/Linux: uc://localhost/{host}/{path} (WebKit supports custom schemes directly)
 *   - Windows: http://uc.localhost/{host}/{path} (WebView2 requires HTTP proxy)
 *
 * We avoid Tauri's convertFileSrc because it applies encodeURIComponent to the
 * entire path, encoding "/" to "%2F" which breaks the protocol handler's routing.
 *
 * Non-uc:// URLs are returned unchanged.
 */
export function resolveUcUrl(ucUrl: string): string {
  const match = UC_PROTOCOL_RE.exec(ucUrl)
  if (!match) {
    return ucUrl
  }
  const path = match[1] // e.g. "thumbnail/rep-1" or "blob/blob-1"
  if (isWindowsPlatform()) {
    return `http://uc.localhost/${path}`
  }
  return `uc://localhost/${path}`
}
