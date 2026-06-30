import { useMemo } from 'react'
import type { ClipboardCodeItem } from '@/lib/clipboard-entry'

const CODE_PREVIEW_LINES = 3

const CODE_KEYWORDS = new Set([
  'abstract',
  'as',
  'async',
  'await',
  'break',
  'case',
  'catch',
  'class',
  'const',
  'continue',
  'def',
  'default',
  'do',
  'elif',
  'else',
  'enum',
  'export',
  'extends',
  'false',
  'final',
  'finally',
  'fn',
  'for',
  'from',
  'func',
  'function',
  'if',
  'impl',
  'import',
  'in',
  'interface',
  'let',
  'match',
  'mut',
  'new',
  'nil',
  'none',
  'null',
  'package',
  'pass',
  'private',
  'protected',
  'pub',
  'public',
  'return',
  'self',
  'static',
  'struct',
  'super',
  'switch',
  'this',
  'throw',
  'trait',
  'true',
  'try',
  'type',
  'typeof',
  'undefined',
  'use',
  'val',
  'var',
  'void',
  'where',
  'while',
  'with',
  'yield',
])

type CodeTone = 'comment' | 'string' | 'number' | 'keyword'

interface CodeSeg {
  text: string
  tone?: CodeTone
}

const TONE_CLASS: Record<CodeTone, string> = {
  comment: 'text-muted-foreground/50 italic',
  string: 'text-emerald-600 dark:text-emerald-400',
  number: 'text-amber-600 dark:text-amber-400',
  keyword: 'text-violet-600 dark:text-violet-400',
}

const FULL_LINE_COMMENT_RE = /^(?:\/\/|#\s|--\s|\*|<!--)/
const CODE_TOKEN_RE =
  /(\/\/.*$|\/\*.*?(?:\*\/|$))|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*'|`(?:[^`\\]|\\.)*`)|(\b\d[\w.]*)|([A-Za-z_$][\w$]*)/g

function tokenizeCodeLine(line: string): CodeSeg[] {
  if (FULL_LINE_COMMENT_RE.test(line.trimStart())) {
    return [{ text: line, tone: 'comment' }]
  }
  const segs: CodeSeg[] = []
  let last = 0
  CODE_TOKEN_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = CODE_TOKEN_RE.exec(line)) !== null) {
    if (m.index > last) segs.push({ text: line.slice(last, m.index) })
    if (m[1]) segs.push({ text: m[1], tone: 'comment' })
    else if (m[2]) segs.push({ text: m[2], tone: 'string' })
    else if (m[3]) segs.push({ text: m[3], tone: 'number' })
    else segs.push(CODE_KEYWORDS.has(m[4]) ? { text: m[4], tone: 'keyword' } : { text: m[4] })
    last = m.index + m[0].length
  }
  if (last < line.length) segs.push({ text: line.slice(last) })
  return segs
}

interface CodeEntryContentProps {
  item: ClipboardCodeItem
}

function CodeEntryContent({ item }: CodeEntryContentProps) {
  const rows = useMemo(() => {
    const trimmed = item.code.replace(/\s+$/, '')
    const allLines = trimmed.length === 0 ? [''] : trimmed.split('\n')
    return allLines
      .slice(0, CODE_PREVIEW_LINES)
      .map((line, i) => ({ num: i + 1, segs: tokenizeCodeLine(line) }))
  }, [item.code])

  return (
    <div className="flex h-full font-mono text-[11px] leading-[1.55]">
      <div className="shrink-0 select-none pr-2.5 text-right tabular-nums text-muted-foreground/25">
        {rows.map(row => (
          <div key={`ln-${row.num}`}>{row.num}</div>
        ))}
      </div>
      <div className="min-w-0 flex-1 overflow-hidden">
        {rows.map(row => (
          <div key={`cl-${row.num}`} className="overflow-hidden whitespace-pre text-foreground/85">
            {row.segs.length === 0
              ? '\u00a0'
              : row.segs.map((seg, j) => (
                  <span
                    key={`s-${row.num}-${j}`}
                    className={seg.tone ? TONE_CLASS[seg.tone] : undefined}
                  >
                    {seg.text}
                  </span>
                ))}
          </div>
        ))}
      </div>
    </div>
  )
}

export default CodeEntryContent
