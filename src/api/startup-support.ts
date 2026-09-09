import { openUrl } from '@tauri-apps/plugin-opener'
import { commands } from '@/lib/ipc'

export const STARTUP_SUPPORT_URL = 'https://github.com/UniClipboard/UniClipboard/issues/new/choose'

export function exportStartupLogs(): Promise<string | null> {
  return commands.exportStartupLogs()
}

export function contactAuthor(): Promise<void> {
  return openUrl(STARTUP_SUPPORT_URL)
}
