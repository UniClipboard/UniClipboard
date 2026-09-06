import InlineTextSummary from '@/components/clipboard/InlineTextSummary'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'

interface CodeEntryContentProps {
  item: ClipboardCodeItem
}

function CodeEntryContent({ item }: CodeEntryContentProps) {
  return (
    <div className="font-mono text-[11px] leading-[1.55] text-foreground/85 line-clamp-2 break-words">
      <InlineTextSummary text={item.code} />
    </div>
  )
}

export default CodeEntryContent
