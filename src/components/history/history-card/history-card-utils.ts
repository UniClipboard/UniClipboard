import {
  Cloud,
  Code,
  ExternalLink,
  File,
  FileText,
  History,
  Image as ImageIcon,
  Laptop,
} from 'lucide-react'
import type React from 'react'
import type { EntrySourceView } from '@/api/tauri-command/clipboard_delivery'
import type {
  ClipboardCodeItem,
  ClipboardEntryTag,
  ClipboardFileItem,
  ClipboardImageItem,
  ClipboardLinkItem,
  ClipboardTextItem,
  DisplayClipboardItem,
} from '@/lib/clipboard-entry'
import { formatFileSize } from '@/utils'

export const TYPE_COLOR: Record<string, string> = {
  text: 'rgb(140,150,160)',
  code: 'rgb(140,120,210)',
  link: 'rgb(70,145,210)',
  image: 'rgb(80,160,110)',
  file: 'rgb(175,140,100)',
  unknown: 'rgb(140,150,160)',
}

export const TYPE_ICONS: Record<string, React.ElementType> = {
  text: FileText,
  code: Code,
  link: ExternalLink,
  image: ImageIcon,
  file: File,
  unknown: FileText,
}

export const TAG_STYLE: Record<
  ClipboardEntryTag,
  { text: string; border: string; background: string }
> = {
  code: {
    text: TYPE_COLOR.code,
    border: 'rgba(140,120,210,0.28)',
    background: 'rgba(140,120,210,0.08)',
  },
  link: {
    text: TYPE_COLOR.link,
    border: 'rgba(70,145,210,0.28)',
    background: 'rgba(70,145,210,0.08)',
  },
}

const SOURCE_CONFIG: Record<EntrySourceView['tag'], { icon: React.ElementType; color: string }> = {
  local: { icon: Laptop, color: 'text-muted-foreground/40' },
  remote: { icon: Cloud, color: 'text-sky-500/60' },
  historical: { icon: History, color: 'text-muted-foreground/30' },
}

export interface SourceDescription {
  Icon: React.ElementType
  color: string
  label: string | null
}

type Translate = (key: string, opts?: Record<string, unknown>) => string

export function fileNameFromPreview(preview: string): string {
  const trimmed = preview.trim().replace(/[/\\]+$/, '')
  const segment = trimmed.split(/[/\\]/).pop() ?? trimmed
  try {
    return decodeURIComponent(segment)
  } catch {
    return segment
  }
}

export function getContentSizeLabel(item: DisplayClipboardItem, t: Translate): string | null {
  if (!item.content) return null
  switch (item.type) {
    case 'text': {
      const textItem = item.content as ClipboardTextItem
      const count = textItem.char_count ?? textItem.display_text.length
      return t('clipboard.preview.charactersCount', { count })
    }
    case 'code': {
      const codeItem = item.content as ClipboardCodeItem
      const count = codeItem.char_count ?? codeItem.code.length
      return t('clipboard.preview.charactersCount', { count })
    }
    case 'link': {
      const link = item.content as ClipboardLinkItem
      return link.domains[0] ?? null
    }
    case 'image':
      return null
    case 'file': {
      const file = item.content as ClipboardFileItem
      const count = file.file_names.length
      if (count > 1) return t('clipboard.preview.filesCount', { count })
      const totalSize = file.file_sizes.filter(s => s >= 0).reduce((a, b) => a + b, 0)
      return totalSize > 0 ? formatFileSize(totalSize) : null
    }
    default:
      return null
  }
}

export function describeSource(
  source: EntrySourceView,
  t: (key: string) => string
): SourceDescription {
  const cfg = SOURCE_CONFIG[source.tag]
  if (source.tag === 'remote') {
    return {
      Icon: cfg.icon,
      color: cfg.color,
      label: source.deviceName ?? source.deviceId.slice(0, 6),
    }
  }
  if (source.tag === 'local') {
    return { Icon: cfg.icon, color: cfg.color, label: t('clipboard.source.local') }
  }
  return { Icon: cfg.icon, color: cfg.color, label: null }
}

export function detectCodeLanguage(code: string): string | null {
  const s = code.slice(0, 1500)
  const has = (re: RegExp) => re.test(s)
  if (has(/^\s*<\?php/)) return 'PHP'
  if (has(/^#!\s*\/.*\b(?:bash|zsh|sh)\b/m) || has(/\b(?:fi|esac|elif)\b/)) return 'Shell'
  if (has(/\bfn\s+\w+/) && has(/\b(?:let\s+mut|impl|pub\s+fn|->\s*\w)/)) return 'Rust'
  if (has(/\bfunc\s+\w+/) && has(/\bpackage\s+\w+/)) return 'Go'
  if (has(/\bdef\s+\w+\s*\(/) || has(/^\s*(?:from\s+\w+\s+import|import\s+\w+)/m)) return 'Python'
  if (has(/:\s*(?:string|number|boolean|void|unknown|any)\b/) || has(/\binterface\s+\w+/)) {
    return 'TypeScript'
  }
  if (has(/\b(?:public|private|protected)\s+(?:static\s+)?(?:class|void|int|String)\b/)) {
    return 'Java'
  }
  if (has(/#include\s*[<"]/)) return 'C++'
  if (has(/^\s*(?:SELECT|INSERT\s+INTO|UPDATE|DELETE\s+FROM|CREATE\s+TABLE)\b/im)) return 'SQL'
  if (has(/^\s*[{[]/) && has(/"\w+"\s*:/) && !has(/\bfunction\b|=>/)) return 'JSON'
  if (has(/<\/[a-z][\w-]*>/i) && has(/<[a-z][\w-]*[\s/>]/i)) return 'HTML'
  if (has(/[.#]?[\w-]+\s*\{[^}]*:[^}]*;/)) return 'CSS'
  if (has(/=>/) || has(/\b(?:const|let|var|function)\b/)) return 'JavaScript'
  return null
}

export function imageTitle(
  label: string,
  loadedDims: { w: number; h: number } | null,
  imageItem?: ClipboardImageItem | null
): string {
  const width = loadedDims?.w ?? (imageItem && imageItem.width > 0 ? imageItem.width : 0)
  const height = loadedDims?.h ?? (imageItem && imageItem.height > 0 ? imageItem.height : 0)
  return width > 0 && height > 0 ? `${label} (${width}×${height})` : label
}
