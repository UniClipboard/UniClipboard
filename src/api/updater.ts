import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { createLogger } from '@/lib/logger'
import { invokeWithTrace } from '@/lib/tauri-command'
import type { UpdateChannel } from '@/types/setting'

const log = createLogger('updater')

/**
 * Broadcast Tauri event name for background download progress.
 * Mirrors `UPDATE_PROGRESS_EVENT` in `src-tauri/.../commands/updater.rs`.
 */
export const UPDATE_PROGRESS_EVENT = 'update-download-progress'

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
  | { event: 'Failed'; data: { error: string } }

export type DownloadPhase = 'idle' | 'available' | 'downloading' | 'ready' | 'installing'

export interface DownloadProgress {
  downloaded: number
  total: number | null
  phase: DownloadPhase
}

/**
 * Backend-provided snapshot used by the frontend to sync state on mount,
 * before attaching the broadcast event listener. Mirrors
 * `DownloadProgressSnapshot` in Rust.
 *
 * `phase` here can only be `idle | available | downloading | ready` —
 * `installing` is a frontend-only transient phase the React layer enters
 * while awaiting `install_update` to restart the app.
 */
export interface DownloadProgressSnapshot {
  phase: 'idle' | 'available' | 'downloading' | 'ready'
  downloaded: number
  total: number | null
  version: string | null
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
    log.error({ err: error }, '检查更新失败')
    throw error
  }
}

/**
 * Trigger background download of the pending update. Progress is broadcast
 * via `UPDATE_PROGRESS_EVENT` — subscribe with `subscribeUpdateProgress`.
 *
 * Resolves when the download completes; rejects on failure or cancellation.
 */
export async function downloadUpdate(): Promise<void> {
  try {
    await invokeWithTrace('download_update', {})
  } catch (error) {
    log.error({ err: error }, '后台下载更新失败')
    throw error
  }
}

/**
 * Cancel an in-flight `downloadUpdate`. No-op if no download is active.
 */
export async function cancelDownload(): Promise<void> {
  try {
    await invokeWithTrace('cancel_download', {})
  } catch (error) {
    log.error({ err: error }, '取消下载更新失败')
    throw error
  }
}

/**
 * Read the current backend update state. Used on Context mount to sync up
 * before attaching the broadcast listener — avoids races where the user
 * navigated away and back during an in-flight download.
 */
export async function getDownloadProgress(): Promise<DownloadProgressSnapshot> {
  try {
    return await invokeWithTrace('get_download_progress', {})
  } catch (error) {
    log.error({ err: error }, '获取下载进度失败')
    throw error
  }
}

/**
 * Subscribe to background download events. Returns an unlisten function;
 * call it on cleanup to detach.
 */
export async function subscribeUpdateProgress(
  onEvent: (event: DownloadEvent) => void
): Promise<UnlistenFn> {
  return listen<DownloadEvent>(UPDATE_PROGRESS_EVENT, message => {
    onEvent(message.payload)
  })
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
      case 'Failed':
        // Surface as thrown error from invoke below; no progress mutation.
        break
    }
  }

  try {
    await invokeWithTrace('install_update', { onEvent })
  } catch (error) {
    log.error({ err: error }, '安装更新失败')
    throw error
  }
}
