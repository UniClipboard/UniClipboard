import InlineTextSummary from '@/components/clipboard/InlineTextSummary'
import type { ClipboardTextItem } from '@/lib/clipboard-entry'

interface TextEntryContentProps {
  item: ClipboardTextItem
}

function TextEntryContent({ item }: TextEntryContentProps) {
  const isMasked = /^[•·*]{6,}$/.test(item.display_text.trim())
  return (
    <div className="text-ui-body text-foreground/85 line-clamp-2 break-words">
      {isMasked ? (
        <span className="text-muted-foreground/70 select-none">{item.display_text}</span>
      ) : (
        <InlineTextSummary text={item.display_text} />
      )}
    </div>
  )
}

export default TextEntryContent
