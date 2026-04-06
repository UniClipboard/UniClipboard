import { useMemo } from 'react'
import { Virtuoso } from 'react-virtuoso'

interface VirtualizedTextProps {
  text: string
  className?: string
}

/**
 * Renders large text using react-virtuoso for viewport-only rendering.
 * Splits text by newlines and only renders visible lines in the DOM.
 */
const VirtualizedText: React.FC<VirtualizedTextProps> = ({ text, className }) => {
  const lines = useMemo(() => text.split('\n'), [text])

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
