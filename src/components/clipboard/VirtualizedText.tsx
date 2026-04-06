import { useMemo } from 'react'
import { Virtuoso } from 'react-virtuoso'

/** Maximum characters per chunk to ensure each virtualized item is small enough. */
const CHUNK_SIZE = 1000

interface VirtualizedTextProps {
  text: string
  className?: string
}

/**
 * Splits text into chunks suitable for virtualization.
 * First splits by newlines, then further splits long lines into fixed-size chunks
 * to handle single-line large text (JSON, base64, etc.).
 */
function splitIntoChunks(text: string): string[] {
  const lines = text.split('\n')
  const chunks: string[] = []
  for (const line of lines) {
    if (line.length <= CHUNK_SIZE) {
      chunks.push(line)
    } else {
      for (let i = 0; i < line.length; i += CHUNK_SIZE) {
        chunks.push(line.slice(i, i + CHUNK_SIZE))
      }
    }
  }
  return chunks
}

/**
 * Renders large text using react-virtuoso for viewport-only rendering.
 * Only visible chunks are rendered in the DOM.
 */
const VirtualizedText: React.FC<VirtualizedTextProps> = ({ text, className }) => {
  const chunks = useMemo(() => splitIntoChunks(text), [text])

  return (
    <Virtuoso
      data={chunks}
      className={className}
      itemContent={(_index, chunk) => (
        <div
          className="whitespace-pre-wrap font-mono text-sm leading-relaxed text-foreground/90"
          style={{ wordBreak: 'break-all' }}
        >
          {chunk || '\u00A0'}
        </div>
      )}
    />
  )
}

export default VirtualizedText
