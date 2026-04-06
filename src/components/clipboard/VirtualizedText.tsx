import { useEffect, useMemo, useRef, useState } from 'react'
import { Virtuoso } from 'react-virtuoso'

/** Number of visual lines per virtualized chunk. */
const LINES_PER_CHUNK = 20

interface VirtualizedTextProps {
  text: string
  className?: string
}

/**
 * Measures how many monospace characters fit per visual line in a container
 * by rendering a known-length test string with the same styles and calculating
 * from the resulting height. This accounts for container width, scrollbar,
 * font size, and any padding automatically.
 */
function measureCharsPerLine(container: HTMLElement): number {
  const test = document.createElement('div')
  test.className = 'whitespace-pre-wrap font-mono text-sm leading-relaxed'
  test.style.cssText = 'word-break:break-all;visibility:hidden;pointer-events:none'

  // Measure single-line height
  test.textContent = 'X'
  container.appendChild(test)
  const oneLineHeight = test.getBoundingClientRect().height

  // Measure with a long string to count how many visual lines it wraps to
  const testLength = 500
  test.textContent = 'X'.repeat(testLength)
  const multiLineHeight = test.getBoundingClientRect().height

  container.removeChild(test)

  if (oneLineHeight <= 0) return 0
  const numLines = Math.round(multiLineHeight / oneLineHeight)
  if (numLines <= 0) return 0

  // Subtract 1 as safety margin for scrollbar width uncertainty
  return Math.max(1, Math.floor(testLength / numLines) - 1)
}

/**
 * Splits text into chunks suitable for virtualization.
 * - Splits by newlines first to preserve paragraph structure.
 * - For long lines that exceed one chunk, splits at visual line boundaries
 *   (multiples of charsPerLine) so each chunk fills exactly LINES_PER_CHUNK
 *   complete visual lines with no partial trailing line.
 */
function splitIntoChunks(text: string, charsPerLine: number): string[] {
  const lines = text.split('\n')
  const chunks: string[] = []
  const chunkCharSize = charsPerLine * LINES_PER_CHUNK

  for (const line of lines) {
    if (line.length <= chunkCharSize) {
      chunks.push(line)
    } else {
      for (let i = 0; i < line.length; i += chunkCharSize) {
        chunks.push(line.slice(i, i + chunkCharSize))
      }
    }
  }
  return chunks
}

/**
 * Renders large text using react-virtuoso for viewport-only rendering.
 * Long lines are split at visual line boundaries to ensure seamless text flow
 * without artificial breaks at chunk edges.
 */
const VirtualizedText: React.FC<VirtualizedTextProps> = ({ text, className }) => {
  const containerRef = useRef<HTMLDivElement>(null)
  const [charsPerLine, setCharsPerLine] = useState(0)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const update = () => {
      const cpl = measureCharsPerLine(el)
      if (cpl > 0) setCharsPerLine(cpl)
    }

    update()

    const observer = new ResizeObserver(() => update())
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  const chunks = useMemo(
    () => (charsPerLine > 0 ? splitIntoChunks(text, charsPerLine) : []),
    [text, charsPerLine]
  )

  return (
    <div ref={containerRef} className={className} style={{ position: 'relative' }}>
      {charsPerLine > 0 && (
        <Virtuoso
          data={chunks}
          style={{ height: '100%' }}
          itemContent={(_index, chunk) => (
            <div
              className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90"
              style={{ wordBreak: 'break-all' }}
            >
              {chunk || '\u00A0'}
            </div>
          )}
        />
      )}
    </div>
  )
}

export default VirtualizedText
