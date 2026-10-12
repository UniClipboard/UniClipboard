import { Fragment } from 'react'

const escape = (term: string) => term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** `text` with every case-insensitive occurrence of `terms` wrapped in `<mark>`. */
export function Highlighted({ text, terms }: { text: string; terms?: string[] }) {
  const words = (terms ?? []).map(term => term.trim()).filter(Boolean)
  if (words.length === 0) return text
  const parts = text.split(new RegExp(`(${words.map(escape).join('|')})`, 'gi'))
  return parts.map((part, index) =>
    index % 2 === 1 ? (
      // eslint-disable-next-line react/no-array-index-key -- split parts are positional
      <mark key={index}>{part}</mark>
    ) : (
      // eslint-disable-next-line react/no-array-index-key -- split parts are positional
      <Fragment key={index}>{part}</Fragment>
    )
  )
}
