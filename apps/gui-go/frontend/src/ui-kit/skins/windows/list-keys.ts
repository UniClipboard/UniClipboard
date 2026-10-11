/** Arrow/Home/End movement in a single-column list; `null` when the key does not move. */
export function listStep(key: string, index: number, count: number): number | null {
  if (count === 0) return null
  if (key === 'ArrowDown') return Math.min(count - 1, index + 1)
  if (key === 'ArrowUp') return Math.max(0, index - 1)
  if (key === 'Home') return 0
  if (key === 'End') return count - 1
  return null
}

/** Ids from `from` to `to` (inclusive) in list order, ending with `to` (the moving end). */
export function selectRange(items: readonly { id: string }[], from: string, to: string): string[] {
  const a = items.findIndex(item => item.id === from)
  const b = items.findIndex(item => item.id === to)
  if (a < 0 || b < 0) return [to]
  const [start, end] = a <= b ? [a, b] : [b, a]
  return [
    ...items
      .slice(start, end + 1)
      .map(item => item.id)
      .filter(id => id !== to),
    to,
  ]
}
