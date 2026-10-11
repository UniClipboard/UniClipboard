import { useRef, type MouseEvent } from 'react'

/** Ids from `from` to `to` (inclusive) in `ids` order, `to` last. */
function range(ids: string[], from: string, to: string) {
  const a = ids.indexOf(from)
  const b = ids.indexOf(to)
  if (a < 0 || b < 0) return [to]
  const [start, end] = a <= b ? [a, b] : [b, a]
  return [...ids.slice(start, end + 1).filter(id => id !== to), to]
}

/**
 * AppKit selection over an ordered collection: a click selects one item, Command-click toggles an
 * item, Shift-click and Shift-arrows extend from the anchor, a secondary click keeps a selection
 * that contains the item. The last id of the selection is where the keyboard moves from.
 */
export function useAnchoredSelection(
  ids: string[],
  selection: string[],
  onSelectionChange: (ids: string[]) => void
) {
  const anchor = useRef<string | null>(null)
  const moveTo = (id: string, extend: boolean) => {
    if (extend && anchor.current) onSelectionChange(range(ids, anchor.current, id))
    else {
      anchor.current = id
      onSelectionChange([id])
    }
  }
  const pointerDown = (event: MouseEvent, id: string) => {
    const selected = selection.includes(id)
    if (event.button === 2 || (event.button === 0 && event.ctrlKey)) {
      if (!selected) {
        anchor.current = id
        onSelectionChange([id])
      }
      return
    }
    if (event.button !== 0) return
    if (event.metaKey) {
      anchor.current = id
      onSelectionChange(selected ? selection.filter(s => s !== id) : [...selection, id])
    } else moveTo(id, event.shiftKey)
  }
  return { moveTo, pointerDown, selectAll: () => onSelectionChange([...ids]) }
}
