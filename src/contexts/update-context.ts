import { createContext } from 'react'
import type { UpdateMetadata, DownloadProgress } from '@/api/updater'
import type { UpdateChannel } from '@/types/setting'

export interface UpdateContextType {
  updateInfo: UpdateMetadata | null
  isCheckingUpdate: boolean
  downloadProgress: DownloadProgress
  checkForUpdates: (channelOverride?: UpdateChannel | null) => Promise<UpdateMetadata | null>
  installUpdate: () => Promise<void>
}

export const UpdateContext = createContext<UpdateContextType | undefined>(undefined)
