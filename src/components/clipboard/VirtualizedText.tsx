import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Virtuoso } from 'react-virtuoso'

/** Lines longer than this trigger single-element windowed rendering. */
const LONG_LINE_THRESHOLD = 50_000

/** Extra lines rendered above/below the viewport for smooth scrolling. */
const VIEWPORT_BUFFER_LINES = 10

interface VirtualizedTextProps {
  text: string
  className?: string
}

/**
 * Measures how many monospace characters fit per visual line and the line
 * height by rendering a test string inside the container with the exact
 * same CSS as the real content.
 */
function measureTextMetrics(
  container: HTMLElement
): { charsPerLine: number; lineHeight: number } | null {
  const test = document.createElement('div')
  test.className = 'whitespace-pre-wrap font-mono text-sm leading-relaxed'
  test.style.cssText = 'word-break:break-all;visibility:hidden;pointer-events:none'

  // Measure single-line height
  test.textContent = 'X'
  container.appendChild(test)
  const lineHeight = test.getBoundingClientRect().height

  // Measure how a known-length string wraps to determine chars per line
  const testLen = 500
  test.textContent = 'X'.repeat(testLen)
  const multiH = test.getBoundingClientRect().height

  container.removeChild(test)

  if (lineHeight <= 0) return null
  const numLines = Math.round(multiH / lineHeight)
  if (numLines <= 0) return null

  return { charsPerLine: Math.floor(testLen / numLines), lineHeight }
}

/**
 * Single-element windowed rendering for text containing very long lines.
 *
 * Instead of splitting text into multiple block-level chunks (which creates
 * visible breaks at chunk boundaries), this renders a SINGLE text element
 * containing only the characters visible in the current viewport. Spacer
 * divs above and below maintain correct scroll height.
 *
 * Because there is only one text element, the browser handles line-breaking
 * naturally — no implementation-level chunk boundaries can leak into the
 * user-visible layout.
 */
const WindowedLongText: React.FC<{ text: string; className?: string }> = ({ text, className }) => {
  const containerRef = useRef<HTMLDivElement>(null)
  const [metrics, setMetrics] = useState<{
    charsPerLine: number
    lineHeight: number
    containerHeight: number
  } | null>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const rafRef = useRef(0)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    const update = () => {
      const m = measureTextMetrics(el)
      if (m) setMetrics({ ...m, containerHeight: el.clientHeight })
    }
    update()

    const obs = new ResizeObserver(() => update())
    obs.observe(el)
    return () => obs.disconnect()
  }, [])

  const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    const st = e.currentTarget.scrollTop
    cancelAnimationFrame(rafRef.current)
    rafRef.current = requestAnimationFrame(() => setScrollTop(st))
  }, [])

  const renderData = useMemo(() => {
    if (!metrics) return null
    const { charsPerLine, lineHeight, containerHeight } = metrics
    if (charsPerLine <= 0) return null

    const totalLines = Math.ceil(text.length / charsPerLine)

    const startLine = Math.max(0, Math.floor(scrollTop / lineHeight) - VIEWPORT_BUFFER_LINES)
    const endLine = Math.min(
      totalLines,
      Math.ceil((scrollTop + containerHeight) / lineHeight) + VIEWPORT_BUFFER_LINES
    )

    const startChar = startLine * charsPerLine
    const endChar = Math.min(text.length, endLine * charsPerLine)

    return {
      topHeight: startLine * lineHeight,
      bottomHeight: Math.max(0, (totalLines - endLine) * lineHeight),
      visibleText: text.slice(startChar, endChar),
    }
  }, [text, metrics, scrollTop])

  return (
    <div
      ref={containerRef}
      className={className}
      style={{ overflow: 'auto' }}
      onScroll={handleScroll}
    >
      {renderData && (
        <>
          <div style={{ height: renderData.topHeight }} />
          <div
            className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90"
            style={{ wordBreak: 'break-all' }}
          >
            {renderData.visibleText}
          </div>
          <div style={{ height: renderData.bottomHeight }} />
        </>
      )}
    </div>
  )
}

/**
 * Renders large text with performance optimization.
 *
 * - Multi-line text (no extremely long lines): uses react-virtuoso with
 *   one item per logical line. Line boundaries are real newlines, so
 *   there are no artificial visual breaks.
 *
 * - Text with very long lines (>50K chars): uses single-element windowed
 *   rendering. Only the viewport-visible portion of text is in the DOM,
 *   inside a single continuous element that the browser line-breaks
 *   naturally. No block-level chunk boundaries exist.
 */
const VirtualizedText: React.FC<VirtualizedTextProps> = ({ text, className }) => {
  const lines = useMemo(() => text.split('\n'), [text])
  const hasLongLine = useMemo(() => lines.some(l => l.length > LONG_LINE_THRESHOLD), [lines])

  if (hasLongLine) {
    return <WindowedLongText text={text} className={className} />
  }

  return (
    <Virtuoso
      data={lines}
      className={className}
      itemContent={(_index, line) => (
        <div
          className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90"
          style={{ wordBreak: 'break-all' }}
        >
          {line || '\u00A0'}
        </div>
      )}
    />
  )
}

export default VirtualizedText
