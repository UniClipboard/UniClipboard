import {
  DownloadEventKind,
  DownloadPhase as HostDownloadPhase,
  type DownloadEvent as HostDownloadEvent,
  type DownloadProgressSnapshot as HostDownloadProgressSnapshot,
} from '@host/models'
import type { UiInstallKind } from '@/api/generated/types.gen'
import { listen, type UnlistenFn } from '@/host/event'
import { commands } from '@/lib/ipc'
import type { UpdateMetadata as GeneratedUpdateMetadata } from '@/lib/ipc'
import { createLogger } from '@/lib/logger'
import type { UpdateChannel } from '@/types/setting'

const log = createLogger('updater')

/**
 * Host event name for background download progress, emitted by the desktop host updater.
 */
export const UPDATE_PROGRESS_EVENT = 'update-download-progress'

/** Host event name carrying the progress of one `install_update` call. */
export const UPDATE_INSTALL_EVENT = 'update-install-progress'

/**
 * Host event name carrying the result of an update check, emitted by the desktop host updater.
 *
 * Payload: `UpdateMetadata | null`.
 */
export const UPDATE_AVAILABLE_EVENT = 'update-available'

// Re-export generated DTO shapes under historical names so existing call
// sites don't have to follow a rename. Generated types are the source of
// truth (see `src/lib/ipc.ts`).
export type UpdateMetadata = GeneratedUpdateMetadata
export type InstallKind = UiInstallKind

export type DownloadPhase = 'idle' | 'available' | 'downloading' | 'ready' | 'installing'

/**
 * One step of a download or install, in the shape the UI switches on. The host sends a flat optional payload
 * (`HostDownloadEvent`, Wails has no tagged unions); `toDownloadEvent` narrows it once so no consumer re-checks it.
 */
export type DownloadEvent =
  | { event: 'Started'; data: { contentLength: number | null } }
  | { event: 'Progress'; data: { chunkLength: number } }
  | { event: 'Finished' }
  | { event: 'Failed'; data: { error: string } }

/** The queryable download state, with the phase as the UI's string union. */
export type DownloadProgressSnapshot = Omit<HostDownloadProgressSnapshot, 'phase'> & {
  phase: DownloadPhase
}

function toDownloadEvent(raw: HostDownloadEvent): DownloadEvent | null {
  switch (raw.event) {
    case DownloadEventKind.DownloadEventStarted:
      return { event: 'Started', data: { contentLength: raw.data?.contentLength ?? null } }
    case DownloadEventKind.DownloadEventProgress:
      return { event: 'Progress', data: { chunkLength: raw.data?.chunkLength ?? 0 } }
    case DownloadEventKind.DownloadEventFinished:
      return { event: 'Finished' }
    case DownloadEventKind.DownloadEventFailed:
      return { event: 'Failed', data: { error: raw.data?.error ?? '' } }
    default:
      return null
  }
}

function toDownloadPhase(phase: HostDownloadPhase): DownloadPhase {
  switch (phase) {
    case HostDownloadPhase.DownloadPhaseAvailable:
      return 'available'
    case HostDownloadPhase.DownloadPhaseDownloading:
      return 'downloading'
    case HostDownloadPhase.DownloadPhaseReady:
      return 'ready'
    default:
      return 'idle'
  }
}

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
 * Open (or focus) the standalone updater window on demand. Used by the
 * daemon-bootstrap error screen to route a "version too old" user straight to
 * the updater. Idempotent on the native side — focuses the window if it is
 * already open (e.g. the scheduler already popped it).
 */
export async function openUpdaterWindow(): Promise<void> {
  try {
    await commands.openUpdaterWindow()
  } catch (error) {
    log.error({ err: error }, 'failed to open updater window')
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
    const snapshot = await commands.getDownloadProgress()
    return { ...snapshot, phase: toDownloadPhase(snapshot.phase) }
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
  return listen<HostDownloadEvent>(UPDATE_PROGRESS_EVENT, message => {
    const event = toDownloadEvent(message.payload)
    if (event) onEvent(event)
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
  let downloaded = 0
  let total: number | null = null

  // The host reports install progress on a typed event that only `install_update` produces. Listen before the call
  // so the first event cannot be missed, and detach whether the call resolves or rejects.
  const unlisten = await listen<HostDownloadEvent>(UPDATE_INSTALL_EVENT, message => {
    const event = toDownloadEvent(message.payload)
    switch (event?.event) {
      case 'Started':
        total = event.data.contentLength
        onProgress?.({ downloaded: 0, total, phase: 'downloading' })
        break
      case 'Progress':
        downloaded += event.data.chunkLength
        onProgress?.({ downloaded, total, phase: 'downloading' })
        break
      case 'Finished':
        onProgress?.({ downloaded, total, phase: 'installing' })
        break
      case 'Failed':
        // Surfaces as the rejection of the call below; no progress mutation.
        break
    }
  })

  try {
    await commands.installUpdate()
  } catch (error) {
    log.error({ err: error }, '安装更新失败')
    throw error
  } finally {
    unlisten()
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
    return toInstallKind(await commands.getInstallKind())
  } catch (error) {
    log.error({ err: error }, '获取安装类型失败')
    throw error
  }
}

export async function skipVersion(version: string): Promise<void> {
  try {
    await commands.skipVersion(version)
  } catch (error) {
    log.error({ err: error }, '跳过版本失败')
    throw error
  }
}

export async function getAutoDownloadUpdate(): Promise<boolean> {
  try {
    return await commands.getAutoDownloadUpdate()
  } catch (error) {
    log.error({ err: error }, '获取自动下载设置失败')
    throw error
  }
}

export async function setAutoDownloadUpdate(enabled: boolean): Promise<void> {
  try {
    await commands.setAutoDownloadUpdate(enabled)
  } catch (error) {
    log.error({ err: error }, '设置自动下载失败')
    throw error
  }
}

function toInstallKind(kind: string): InstallKind {
  switch (kind) {
    case 'macos':
    case 'windows':
    case 'windowsportable':
    case 'appimage':
    case 'deb':
    case 'rpm':
      return kind
    default:
      return 'unknown'
  }
}
