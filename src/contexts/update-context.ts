import { createContext } from 'react'
import type { UpdateMetadata, DownloadProgress } from '@/api/updater'

export interface UpdateContextType {
  updateInfo: UpdateMetadata | null
  isCheckingUpdate: boolean
  downloadProgress: DownloadProgress
  checkForUpdates: () => Promise<UpdateMetadata | null>
  installUpdate: () => Promise<void>
}

export const UpdateContext = createContext<UpdateContextType | undefined>(undefined)
