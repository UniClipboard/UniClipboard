import type { ClipboardItemResponse } from '@/api/clipboardItems'

export type ItemType = 'text' | 'image' | 'link' | 'code' | 'file' | 'unknown'

export function resolveItemType(item: ClipboardItemResponse): ItemType {
  if (item.item.image) return 'image'
  if (item.item.link) return 'link'
  if (item.item.file) return 'file'
  if (item.item.code) return 'code'
  if (item.item.text) return 'text'
  return 'unknown'
}

export function getItemPreview(item: ClipboardItemResponse): string {
  switch (resolveItemType(item)) {
    case 'image':
      return 'Image'
    case 'link':
      return item.item.link?.urls[0] ?? ''
    case 'file':
      return item.item.file?.file_names[0] ?? ''
    case 'code':
      return item.item.code?.code ?? ''
    case 'text':
      return item.item.text?.display_text ?? ''
    default:
      return ''
  }
}

export function formatRelativeTime(timestampMs: number): string {
  const diffMs = Date.now() - timestampMs
  const diffMins = Math.round(diffMs / 60000)

  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m`
  if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h`
  return `${Math.floor(diffMins / 1440)}d`
}
