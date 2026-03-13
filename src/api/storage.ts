import { invokeWithTrace } from '@/lib/tauri-command'

export interface StorageStats {
  databaseBytes: number
  vaultBytes: number
  cacheBytes: number
  logsBytes: number
  totalBytes: number
  dataDir: string
}

export async function getStorageStats(): Promise<StorageStats> {
  return await invokeWithTrace<StorageStats>('get_storage_stats')
}

export async function clearCache(): Promise<void> {
  return await invokeWithTrace('clear_cache')
}

export async function clearAllClipboardHistory(): Promise<void> {
  return await invokeWithTrace('clear_all_clipboard_history')
}

export async function openDataDirectory(): Promise<void> {
  return await invokeWithTrace('open_data_directory')
}
