/**
 * Clipboard events from backend
 */
export interface ClipboardEvent {
  type: 'NewContent' | 'Deleted'
  entry_id?: string
  preview?: string
  origin?: 'local' | 'remote'
}
