import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'

/**
 * Resolve a clipboard entry's image into a displayable `<img src>`, cached so
 * scrolling a virtualized list back to a row doesn't re-hit the daemon.
 */
export function useResourceImageUrl(entryId: string, enabled = true): string | null {
  return useBlobImageObjectUrl(
    `/clipboard/entries/${encodeURIComponent(entryId)}/thumbnail`,
    enabled
  )
}
