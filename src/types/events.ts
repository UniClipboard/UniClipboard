/**
 * Clipboard events from backend
 */
export interface ClipboardEvent {
  type: 'NewContent' | 'Deleted'
  entry_id?: string
  preview?: string
}

/**
 * Setting changed event data
 */
export interface SettingChangedEvent {
  settingJson: string
  timestamp: number
}
