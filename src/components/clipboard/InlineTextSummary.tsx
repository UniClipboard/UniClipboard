import { Fragment } from 'react'

interface InlineTextSummaryProps {
  text: string
}

function InlineTextSummary({ text }: InlineTextSummaryProps) {
  const lines = Array.from(text.matchAll(/[^\r\n\u2028\u2029]+/gu)).flatMap(match => {
    const content = match[0].trim()
    return content ? [{ content, offset: match.index }] : []
  })

  return (
    <>
      {lines.map((line, index) => (
        <Fragment key={line.offset}>
          {index > 0 && (
            <>
              {' '}
              <span aria-hidden="true" className="text-[0.8em] opacity-45">
                ↵
              </span>{' '}
            </>
          )}
          {line.content}
        </Fragment>
      ))}
    </>
  )
}

export default InlineTextSummary
