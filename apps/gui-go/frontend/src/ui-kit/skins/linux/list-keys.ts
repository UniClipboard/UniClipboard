/** Arrow/Home/End movement of a single-column list; `null` when the key does not move. */
export function listStep(key: string, index: number, count: number): number | null {
  if (count === 0) return null
  if (key === 'ArrowDown') return Math.min(count - 1, index + 1)
  if (key === 'ArrowUp') return Math.max(0, index - 1)
  if (key === 'Home') return 0
  if (key === 'End') return count - 1
  return null
}
