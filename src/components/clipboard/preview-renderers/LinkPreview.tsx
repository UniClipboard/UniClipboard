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
    <div className="space-y-4 p-8">
      {item.urls.map((url, index) => (
        <button
          key={`${url}-${index}`}
          type="button"
          className="group flex w-full items-center gap-3 rounded-xl border border-border/20 bg-muted/10 p-4 text-left transition-all hover:border-primary/30 hover:bg-muted/20"
          onClick={() => openUrl(url).catch(err => log.error({ err }, 'Failed to open URL'))}
        >
          <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary transition-transform group-hover:scale-110">
            <ExternalLink size={18} />
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold text-foreground/90">{url}</div>
            {item.domains[index] && (
              <div className="mt-0.5 text-xs text-muted-foreground/70">{item.domains[index]}</div>
            )}
          </div>
        </button>
      ))}
    </div>
  )
}

export default LinkPreview
