import type { ClipboardTextItem } from '@/lib/clipboard-entry'

interface TextEntryContentProps {
  item: ClipboardTextItem
}

function TextEntryContent({ item }: TextEntryContentProps) {
  const isMasked = /^[•·*]{6,}$/.test(item.display_text.trim())
  return (
    <div className="text-[13px] leading-[1.55] text-foreground/85 line-clamp-2">
      {isMasked ? (
        <span className="tracking-[0.12em] text-muted-foreground/70 select-none">
          {item.display_text}
        </span>
      ) : (
        item.display_text
      )}
    </div>
  )
}

export default TextEntryContent
