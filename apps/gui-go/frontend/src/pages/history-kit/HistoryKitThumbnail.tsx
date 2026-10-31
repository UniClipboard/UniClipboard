import { useResourceImageUrl } from '@/components/history/history-card/useResourceImageUrl'
import { useInView } from '@/hooks/useInView'
import { KindBadge } from '@/ui-kit'

/** An image row's thumbnail, decoded from the daemon's resource bytes. */
export function HistoryKitThumbnail({ entryId }: { entryId: string }) {
  const [ref, visible] = useInView<HTMLSpanElement>()
  const url = useResourceImageUrl(entryId, visible)
  return (
    <span ref={ref} className="inline-flex">
      <KindBadge kind="image" label="" variant="tile" imageSrc={url ?? undefined} />
    </span>
  )
}
