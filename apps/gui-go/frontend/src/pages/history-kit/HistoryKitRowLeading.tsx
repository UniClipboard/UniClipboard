import { KIND_GLYPH, type HistoryKind } from '@/components/history/list/history-list-kind'
import { KindBadge } from '@/ui-kit'
import { HistoryKitThumbnail } from './HistoryKitThumbnail'

/** The type mark of a history row; an image row shows its thumbnail from the daemon. */
export function HistoryKitRowLeading({ entryId, kind }: { entryId: string; kind: HistoryKind }) {
  return kind === 'image' ? (
    <HistoryKitThumbnail entryId={entryId} />
  ) : (
    <KindBadge kind={kind} label={KIND_GLYPH[kind]} variant="tile" />
  )
}
