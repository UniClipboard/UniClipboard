import { useResourceImageUrl } from '@/components/history/history-card/useResourceImageUrl'
import { KindBadge } from '@/ui-kit'

/** An image row's thumbnail, decoded from the daemon's resource bytes. */
export function HistoryKitThumbnail({ entryId }: { entryId: string }) {
  const url = useResourceImageUrl(entryId)
  return <KindBadge kind="image" label="" variant="tile" imageSrc={url ?? undefined} />
}
