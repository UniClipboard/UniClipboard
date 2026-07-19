import { openUrl } from '@tauri-apps/plugin-opener'
import { ExternalLink } from 'lucide-react'
import React from 'react'
import type { ClipboardLinkItem } from '@/lib/clipboard-entry'
import { createLogger } from '@/lib/logger'

const log = createLogger('clipboard-preview')

interface LinkPreviewProps {
  item: ClipboardLinkItem
}

const LinkPreview: React.FC<LinkPreviewProps> = ({ item }) => {
  return (
    <div className="divide-y divide-border/30 px-8 py-4">
      {item.urls.map((url, index) => (
        <button
          key={`${url}-${index}`}
          type="button"
          className="group flex w-full items-start gap-2 py-3 text-left outline-none focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
          onClick={() => openUrl(url).catch(err => log.error({ err }, 'Failed to open URL'))}
        >
          <ExternalLink className="mt-1 size-3.5 shrink-0 text-muted-foreground/60 transition-colors group-hover:text-foreground/80" />
          <span className="break-all whitespace-normal font-mono text-sm leading-relaxed text-foreground/80 underline decoration-border underline-offset-4 transition-colors group-hover:text-foreground group-hover:decoration-foreground/40">
            {url}
          </span>
        </button>
      ))}
    </div>
  )
}

export default LinkPreview
