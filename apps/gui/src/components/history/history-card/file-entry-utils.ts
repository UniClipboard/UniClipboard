import type { ClipboardFileItem } from '@/lib/clipboard-entry'
import { isImageFileName } from '@/lib/clipboard-utils'

export function isSingleImageFile(item: ClipboardFileItem): boolean {
  return item.file_names.length === 1 && isImageFileName(item.file_names[0] ?? '')
}
