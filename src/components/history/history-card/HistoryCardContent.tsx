import type {
  ClipboardCodeItem,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import CodeEntryContent from './CodeEntryContent'
import { isSingleImageFile } from './file-entry-utils'
import FileEntryContent from './FileEntryContent'
import { fileNameFromPreview } from './history-card-utils'
import ImageEntryContent from './ImageEntryContent'
import ImageFileEntryContent from './ImageFileEntryContent'
import LinkEntryContent from './LinkEntryContent'
import TextEntryContent from './TextEntryContent'

interface HistoryCardContentProps {
  item: DisplayClipboardItem
}

function HistoryCardContent({ item }: HistoryCardContentProps) {
  if (item.type === 'image') {
    return (
      <ImageEntryContent entryId={item.id} imageItem={item.content as ClipboardImageItem | null} />
    )
  }
  if (!item.content) {
    if (item.type === 'file' && item.textPreview) {
      const fileItem: ClipboardFileItem = {
        file_names: [fileNameFromPreview(item.textPreview)],
        file_sizes: [-1],
      }
      return isSingleImageFile(fileItem) ? (
        <ImageFileEntryContent item={fileItem} entryId={item.id} />
      ) : (
        <FileEntryContent item={fileItem} />
      )
    }
    if (item.type === 'code' && item.textPreview) {
      return <CodeEntryContent item={{ code: item.textPreview }} />
    }
    return item.textPreview ? (
      <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-2 break-words whitespace-pre-wrap">
        {item.textPreview}
      </div>
    ) : null
  }
  switch (item.type) {
    case 'text':
      return <TextEntryContent item={item.content as ClipboardTextItem} />
    case 'code':
      return <CodeEntryContent item={item.content as ClipboardCodeItem} />
    case 'link':
      return <LinkEntryContent item={item.content as ClipboardLinkItem} />
    case 'file': {
      const fileItem = item.content as ClipboardFileItem
      return isSingleImageFile(fileItem) ? (
        <ImageFileEntryContent item={fileItem} entryId={item.id} />
      ) : (
        <FileEntryContent item={fileItem} />
      )
    }
    default:
      return item.textPreview ? (
        <div className="text-[13px] text-muted-foreground/70 line-clamp-3">{item.textPreview}</div>
      ) : null
  }
}

export default HistoryCardContent
