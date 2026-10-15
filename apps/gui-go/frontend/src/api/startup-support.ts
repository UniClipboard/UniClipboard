import { openUrl } from '@/host/opener'
import { commands, type ExportStartupLogsResult } from '@/lib/ipc'
import { STARTUP_SUPPORT_URL } from './startup-support-url'

// The URL lives in its own module so the startup screen can show it without loading IPC.
export { STARTUP_SUPPORT_URL }

export function exportStartupLogs(): Promise<ExportStartupLogsResult | null> {
  return commands.exportStartupLogs()
}

export function contactAuthor(): Promise<void> {
  return openUrl(STARTUP_SUPPORT_URL)
}
