import React from 'react'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { countCodeLines, resolveCodePreviewText } from './codePreviewUtils'

/** `editor`: line-numbered, unwrapped pane. `block`: the macOS detail column's
 * dark, wrapped code block (HDetail.dc.html), no gutter. */
export type CodePreviewVariant = 'editor' | 'block'

interface CodePreviewProps {
  item: ClipboardCodeItem
  preview: ClipboardPreviewData | null
  variant?: CodePreviewVariant
}

const CodePreview: React.FC<CodePreviewProps> = ({ item, preview, variant = 'editor' }) => {
  const code = resolveCodePreviewText(item.code, preview)

  if (variant === 'block') {
    return (
      <div
        data-testid="code-preview"
        className="h-full overflow-auto bg-zinc-900 font-mono text-ui-body-relaxed text-zinc-300 dark:bg-black/60"
      >
        <pre className="selectable px-5 py-4.5 break-all whitespace-pre-wrap">
          <code>{code}</code>
        </pre>
      </div>
    )
  }

  const lineCount = countCodeLines(code)
  return (
    <div
      data-testid="code-preview"
      className="h-full overflow-auto bg-card font-mono text-ui-body text-foreground/85"
    >
      <div className="flex w-max min-w-full">
        <div
          aria-hidden
          className="sticky left-0 z-10 shrink-0 select-none bg-card py-5 pl-[var(--clipboard-preview-inset,0.75rem)] pr-2 text-right tabular-nums text-muted-foreground/35"
        >
          {Array.from({ length: lineCount }, (_, i) => (
            <div key={i} className="flex h-(--line-height-body) items-center justify-end">
              <span className="text-ui-caption">{i + 1}</span>
            </div>
          ))}
        </div>
        <pre className="selectable shrink-0 px-4 py-5">
          <code>{code}</code>
        </pre>
      </div>
    </div>
  )
}

export default CodePreview
