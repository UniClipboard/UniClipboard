import React from 'react'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'
import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

interface CodePreviewProps {
  item: ClipboardCodeItem
  preview: ClipboardPreviewData | null
}

const CodePreview: React.FC<CodePreviewProps> = ({ item, preview }) => {
  const code = preview?.contentType === 'text' ? (preview.textContent ?? item.code) : item.code

  return (
    <div className="p-6">
      <div className="group relative">
        <div className="pointer-events-none absolute inset-0 rounded-full bg-primary/5 opacity-0 blur-xl transition-opacity group-hover:opacity-100" />
        <pre className="relative overflow-auto rounded-xl border border-white/5 bg-[#0d1117] p-5 font-mono text-[13px] leading-relaxed text-blue-100/90 shadow-2xl">
          <code>{code}</code>
        </pre>
      </div>
    </div>
  )
}

export default CodePreview
