import type { ClipboardPreviewData } from '@/lib/clipboard-preview-cache'

export function resolveCodePreviewText(
  fallback: string,
  preview: ClipboardPreviewData | null
): string {
  return preview?.contentType === 'text' ? (preview.textContent ?? fallback) : fallback
}

export function countCodeLines(code: string): number {
  return code.replace(/\n$/, '').split('\n').length
}
