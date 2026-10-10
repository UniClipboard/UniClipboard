/**
 * Clipboard events from backend
 */
export interface ClipboardEvent {
  type: 'NewContent' | 'Deleted'
  entry_id?: string
  preview?: string
  origin?: 'local' | 'remote'
}

/**
 * Setting changed event data
 */
export interface SettingChangedEvent {
  settingJson: string
  timestamp: number
}
