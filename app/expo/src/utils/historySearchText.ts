/**
 * History search folds ASCII A-Z only, matching SQLite's built-in LOWER.
 * Non-ASCII characters keep their case; no locale or Unicode normalization.
 * Keep this contract for both SQL parameters and incremental text/filename search.
 */
export function foldHistorySearchText(value: string): string {
  return value.replace(/[A-Z]/g, (character) => character.toLowerCase());
}
