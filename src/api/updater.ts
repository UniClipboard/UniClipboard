import { Channel } from '@tauri-apps/api/core'
import { invokeWithTrace } from '@/lib/tauri-command'
import type { UpdateChannel } from '@/types/setting'

export interface UpdateMetadata {
  version: string
  currentVersion: string
  body?: string
  date?: string
}

export type DownloadEvent =
  | { event: 'Started'; data: { contentLength: number | null } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished' }

export interface DownloadProgress {
  downloaded: number
  total: number | null
  phase: 'idle' | 'downloading' | 'installing'
}

/**
 * 检查更新
 * @param channel 可选的更新频道，null 表示自动检测
 * @returns Promise，返回更新信息或 null（无更新）
 */
export async function checkForUpdate(
  channel?: UpdateChannel | null
): Promise<UpdateMetadata | null> {
  try {
    return await invokeWithTrace('check_for_update', { channel: channel ?? null })
  } catch (error) {
    console.error('检查更新失败:', error)
    throw error
  }
}

/**
 * 安装更新
 * @param onProgress 可选的进度回调
 * @returns Promise，安装完成后应用重启
 */
export async function installUpdate(
  onProgress?: (progress: DownloadProgress) => void
): Promise<void> {
  const onEvent = new Channel<DownloadEvent>()
  let downloaded = 0
  let total: number | null = null

  onEvent.onmessage = message => {
    switch (message.event) {
      case 'Started':
        total = message.data.contentLength
        onProgress?.({ downloaded: 0, total, phase: 'downloading' })
        break
      case 'Progress':
        downloaded += message.data.chunkLength
        onProgress?.({ downloaded, total, phase: 'downloading' })
        break
      case 'Finished':
        onProgress?.({ downloaded, total, phase: 'installing' })
        break
    }
  }

  try {
    await invokeWithTrace('install_update', { onEvent })
  } catch (error) {
    console.error('安装更新失败:', error)
    throw error
  }
}
