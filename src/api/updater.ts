import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { commands } from '@/lib/ipc'
import type {
  DownloadEvent as GeneratedDownloadEvent,
  DownloadProgressSnapshot as GeneratedDownloadProgressSnapshot,
  InstallKind as GeneratedInstallKind,
  UpdateMetadata as GeneratedUpdateMetadata,
} from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'

const log = createLogger('updater')

/**
 * Broadcast Tauri event name for background download progress.
 * Mirrors `UPDATE_PROGRESS_EVENT` in `src-tauri/.../commands/updater.rs`.
 */
export const UPDATE_PROGRESS_EVENT = 'update-download-progress'

/**
 * Broadcast Tauri event name carrying the result of `do_check_for_update`.
 * Mirrors `UPDATE_AVAILABLE_EVENT` in `src-tauri/.../commands/updater.rs`.
 *
 * Payload: `UpdateMetadata | null`.
 */
export const UPDATE_AVAILABLE_EVENT = 'update-available'

// Re-export generated DTO shapes under historical names so existing call
// sites don't have to follow a rename. Generated types are the source of
// truth (see `src/lib/ipc.ts`).
export type UpdateMetadata = GeneratedUpdateMetadata
export type DownloadEvent = GeneratedDownloadEvent
export type DownloadProgressSnapshot = GeneratedDownloadProgressSnapshot
export type InstallKind = GeneratedInstallKind

export type DownloadPhase = 'idle' | 'available' | 'downloading' | 'ready' | 'installing'

export interface DownloadProgress {
  downloaded: number
  total: number | null
  phase: DownloadPhase
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
    return await commands.checkForUpdate(channel ?? null)
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
    await commands.downloadUpdate()
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
    await commands.cancelDownload()
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
    return await commands.getDownloadProgress()
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
 * Subscribe to "update detected" broadcasts emitted by `do_check_for_update`
 * on every transition (scheduler or manual). Payload is `UpdateMetadata`
 * when an update was found (Available / preserved Ready), `null` when the
 * check reported UpToDate.
 *
 * Without this listener the UI indicator would never reflect a
 * scheduler-detected update — Phase 6A removed the frontend's startup
 * check, leaving mount-time `getDownloadProgress` as the only sync point.
 */
export async function subscribeUpdateAvailable(
  onEvent: (meta: UpdateMetadata | null) => void
): Promise<UnlistenFn> {
  return listen<UpdateMetadata | null>(UPDATE_AVAILABLE_EVENT, message => {
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
    await commands.installUpdate(onEvent)
  } catch (error) {
    log.error({ err: error }, '安装更新失败')
    throw error
  }
}

/**
 * Probe how the current binary was installed. Cached on the backend after the
 * first call, so it's safe to invoke unconditionally on mount.
 *
 * Used to route Linux deb/rpm users to their system package manager instead
 * of the in-app updater (which Tauri only supports for AppImage on Linux).
 */
export async function getInstallKind(): Promise<InstallKind> {
  try {
    return await commands.getInstallKind()
  } catch (error) {
    log.error({ err: error }, '获取安装类型失败')
    throw error
  }
}
