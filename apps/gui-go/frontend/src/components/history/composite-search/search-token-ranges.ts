import {
  buildCandidates,
  parseBuffer,
  type CandidateContext,
  type ParsedBuffer,
} from '@/components/history/composite-search/composite-search-model'

export interface SearchSelection {
  start: number
  end: number
}

export interface SearchTokenRange extends SearchSelection {
  parsed: Extract<ParsedBuffer, { kind: 'token' }>
}

/** Locate editable filter words, leaving quoted/escaped literal text untouched. */
export function searchTokenRanges(buffer: string) {
  const ranges: SearchTokenRange[] = []
  const words = /(?:[^\s"'\\]|\\[\s\S]|"(?:[^"\\]|\\[\s\S])*"?|'(?:[^'\\]|\\[\s\S])*'?)+/g
  for (const match of buffer.matchAll(words)) {
    const raw = match[0]
    if (match.index > 0 && !/\s/.test(buffer[match.index - 1])) continue
    if (buffer[match.index + raw.length] === '\\') continue
    // Quotes around the entire word and a leading escape explicitly mean literal text.
    if (/^["'\\]/.test(raw)) continue
    const parsed = parseBuffer(raw)
    if (parsed.kind !== 'token') continue
    // A path or URL is never a content-type completion.
    if (parsed.dimension === 'type' && /[/\\]/.test(parsed.partial)) continue
    const partial = parsed.partial.replace(/^(["'])([\s\S]*)\1$/, (_match, _quote, value: string) =>
      value.replace(/\\(["'\\])/g, '$1')
    )
    ranges.push({
      start: match.index,
      end: match.index + raw.length,
      parsed: { ...parsed, partial, committed: /\s/.test(buffer.charAt(match.index + raw.length)) },
    })
  }
  return ranges
}

/** The active completion and the text query sent while its suggestions are open. */
export function resolveSearchInput(
  buffer: string,
  selection: SearchSelection,
  context: CandidateContext
) {
  const range = activeSearchToken(buffer, selection)
  const typed = range?.parsed
  const candidates = typed ? buildCandidates(typed.dimension, typed.partial, context) : []
  const parsed: ParsedBuffer =
    typed && (!typed.partial || candidates.length > 0) ? typed : { kind: 'query', text: buffer }
  const query =
    parsed.kind === 'token' && range ? replaceSearchRange(buffer, range).buffer.trim() : buffer
  return { range, typed, parsed, candidates, query }
}

export function activeSearchToken(buffer: string, selection: SearchSelection) {
  return searchTokenRanges(buffer).find(
    range => range.start <= selection.start && range.end >= selection.end
  )
}

/** Remove only a condition's range; retain the text on both sides and its caret position. */
export function replaceSearchRange(buffer: string, range: SearchSelection, replacement = '') {
  const text = buffer.slice(0, range.start) + replacement + buffer.slice(range.end)
  if (!text.trim()) return { buffer: '', selection: { start: 0, end: 0 } }
  return {
    buffer: text,
    selection: { start: range.start + replacement.length, end: range.start + replacement.length },
  }
}
