import { useBlobImageObjectUrl } from '@/hooks/useBlobImageObjectUrl'
import { useResourceImageDescriptor } from '@/hooks/useResourceImageDescriptor'

/**
 * Resolve a clipboard entry's image into a displayable `<img src>`, cached so
 * scrolling a virtualized list back to a row doesn't re-hit the daemon.
 */
export function useResourceImageUrl(entryId: string): string | null {
  const descriptor = useResourceImageDescriptor(entryId)
  return useBlobImageObjectUrl(descriptor)
}
