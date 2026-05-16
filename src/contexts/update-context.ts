import { createContext } from 'react'
import type { DownloadPhase, DownloadProgress, InstallKind, UpdateMetadata } from '@/api/updater'
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
  /**
   * How the running binary was installed. `null` while detection is in
   * flight (mount race) — treat as "unknown / let the in-app flow run".
   * For `deb`/`rpm`, callers must route the user to the system package
   * manager instead of invoking download/install.
   */
  installKind: InstallKind | null
  /**
   * Convenience: `true` when the binary is owned by a system package
   * manager (deb/rpm). In-app download & install must be suppressed.
   */
  isSystemManaged: boolean
}

export const UpdateContext = createContext<UpdateContextType | undefined>(undefined)
