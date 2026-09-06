import React from 'react'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { countCodeLines, resolveCodePreviewText } from './codePreviewUtils'

interface CodePreviewProps {
  item: ClipboardCodeItem
  preview: ClipboardPreviewData | null
}

const CodePreview: React.FC<CodePreviewProps> = ({ item, preview }) => {
  const code = resolveCodePreviewText(item.code, preview)
  const lineCount = countCodeLines(code)

  return (
    <div
      data-testid="code-preview"
      className="h-full overflow-auto bg-transparent font-mono text-[13px] leading-relaxed text-foreground/85"
    >
      <div className="flex w-max min-w-full">
        <div
          aria-hidden
          className="sticky left-0 z-10 shrink-0 select-none border-r border-border/25 bg-muted/20 py-5 pl-3 pr-2 text-right tabular-nums text-muted-foreground/35"
        >
          {Array.from({ length: lineCount }, (_, i) => (
            <div key={i}>{i + 1}</div>
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
