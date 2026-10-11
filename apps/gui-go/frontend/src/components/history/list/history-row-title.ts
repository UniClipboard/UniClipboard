import {
  fileNameFromPreview,
  imageTitle,
} from '@/components/history/history-card/history-card-utils'
import type {
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { linkItemFromTextContent } from '@/lib/clipboard-utils'
import type { HistoryKind } from './history-list-kind'

/** One-line title for a row: the text itself, the link, the file name, or the
 * image label with its dimensions. */
export function rowTitle(
  item: DisplayClipboardItem,
  kind: HistoryKind,
  imageLabel: string
): string {
  if (kind === 'image') return imageTitle(imageLabel, null, item.content as ClipboardImageItem)
  if (kind === 'file') {
    const file = item.content as ClipboardFileItem | null
    if (file?.file_names.length) {
      const extra = file.file_names.length - 1
      return extra > 0 ? `${file.file_names[0]} +${extra}` : file.file_names[0]
    }
    return item.textPreview ? fileNameFromPreview(item.textPreview) : ''
  }
  const text = item.content as ClipboardTextItem | null
  if (kind === 'link' && text) {
    const link = linkItemFromTextContent(text)
    if (link?.urls[0]) return link.urls[0]
  }
  return text?.display_text ?? item.textPreview ?? ''
}
