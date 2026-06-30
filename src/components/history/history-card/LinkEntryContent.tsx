import { ExternalLink } from 'lucide-react'
import type { ClipboardLinkItem } from '@/lib/clipboard-entry'

interface LinkEntryContentProps {
  item: ClipboardLinkItem
}

function LinkEntryContent({ item }: LinkEntryContentProps) {
  const url = item.urls[0] ?? ''
  const domain = item.domains[0] ?? ''
  let title = url
  try {
    const u = new URL(url)
    title = u.pathname === '/' ? u.hostname : `${u.hostname}${u.pathname}`
  } catch {
    /* keep raw url */
  }
  return (
    <div className="space-y-0.5">
      <div className="text-[13px] font-medium text-foreground/85 leading-snug line-clamp-2">
        {title}
      </div>
      <div className="flex items-center gap-1 text-[11px] text-muted-foreground/70">
        <ExternalLink className="size-[10px] shrink-0" />
        <span className="truncate">{domain}</span>
      </div>
    </div>
  )
}

export default LinkEntryContent
