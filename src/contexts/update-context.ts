import { createContext } from 'react'
import type { DownloadPhase, DownloadProgress, UpdateMetadata } from '@/api/updater'
import type { UpdateChannel } from '@/types/setting'

/**
 * Frontend-facing update state. Combines backend `DownloadProgressSnapshot`
 * with a transient client-only `installing` phase entered while waiting for
 * `install_update` to restart the app.
 */
export interface UpdateState {
  phase: DownloadPhase
  info: UpdateMetadata | null
  downloaded: number
  total: number | null
}

export interface UpdateContextType {
  /** Rich state machine — preferred for new code. */
  state: UpdateState

  isCheckingUpdate: boolean

  checkForUpdates: (channelOverride?: UpdateChannel | null) => Promise<UpdateMetadata | null>
  downloadUpdate: () => Promise<void>
  cancelDownload: () => Promise<void>
  installUpdate: () => Promise<void>

  /** Convenience alias for `state.info`. */
  updateInfo: UpdateMetadata | null
  /** Convenience flat view of `state.{phase, downloaded, total}`. */
  downloadProgress: DownloadProgress
}

export const UpdateContext = createContext<UpdateContextType | undefined>(undefined)
