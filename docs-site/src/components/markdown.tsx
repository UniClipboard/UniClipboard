'use client'

import { createMarkdownRenderer } from 'fumadocs-core/content/md'

const MarkdownContent = createMarkdownRenderer().Markdown

export function Markdown({ text }: { text: string }) {
  return <MarkdownContent>{text}</MarkdownContent>
}
