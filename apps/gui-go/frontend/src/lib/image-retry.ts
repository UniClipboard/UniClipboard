/** Shared capped backoff for visible and buffered image requests. */
export function imageRetryDelay(attempt: number): number {
  return Math.min(500 * 2 ** Math.min(attempt, 4), 8000) + Math.random() * 500
}
