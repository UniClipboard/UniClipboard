import React from 'react'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

interface CodePreviewProps {
  item: ClipboardCodeItem
  preview: ClipboardPreviewData | null
}

const CodePreview: React.FC<CodePreviewProps> = ({ item, preview }) => {
  const code = preview?.contentType === 'text' ? (preview.textContent ?? item.code) : item.code
  // Drop a single trailing newline so the gutter doesn't render a phantom last line.
  const lineCount = code.replace(/\n$/, '').split('\n').length

  return (
    <div className="h-full p-6">
      <div className="group relative h-full">
        <div className="pointer-events-none absolute inset-0 rounded-xl bg-primary/5 opacity-0 blur-xl transition-opacity group-hover:opacity-100" />
        {/* Single scroll container: vertical scroll grows the whole grid, horizontal
            scroll slides the code while the sticky gutter stays pinned at left. */}
        <div className="relative h-full overflow-auto rounded-xl border border-white/5 bg-[#0d1117] font-mono text-[13px] leading-relaxed text-blue-100/90 shadow-2xl">
          <div className="flex w-max min-w-full">
            <div
              aria-hidden
              className="sticky left-0 z-10 shrink-0 select-none border-r border-white/5 bg-[#0d1117] py-5 pl-3 pr-2 text-right tabular-nums text-blue-100/25"
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
      </div>
    </div>
  )
}

export default CodePreview
