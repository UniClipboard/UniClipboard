import type { EntryPageStatus } from '@/ui-kit'

/** The startup screen's category tone as the kit's status tone. */
export function statusTone(
  tone: 'default' | 'destructive' | 'success'
): NonNullable<EntryPageStatus['tone']> {
  if (tone === 'destructive') return 'critical'
  if (tone === 'success') return 'positive'
  return 'neutral'
}

/** `m:ss`, as the existing startup screen shows elapsed time. */
export function formatElapsed(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
}
