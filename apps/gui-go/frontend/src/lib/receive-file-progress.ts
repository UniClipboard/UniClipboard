import type { ClipboardFileItem } from '@/lib/clipboard-entry'
import type { parseFileItemsFromUriList } from '@/lib/clipboard-utils'
import type { ReceiveItem, ReceiveSnapshot } from '@/lib/receive-items'

export type ReceiveHistoryFiles = ReturnType<typeof parseFileItemsFromUriList>

/** This key changes when history positions can change, but not for byte-only updates. */
export function receiveFileMappingKey(snapshot: ReceiveSnapshot): string {
  return JSON.stringify([
    snapshot.entryId,
    snapshot.attemptId,
    snapshot.snapshotVersion?.producerId,
    snapshot.items.map(item => [
      item.itemId,
      item.content,
      item.historyFileIndex,
      item.state === 'completed',
    ]),
  ])
}

/** Compare decimal revisions without losing their unsigned 64-bit precision. */
export function isOlderReceiveSnapshot(next: ReceiveSnapshot, stored: ReceiveSnapshot): boolean {
  const a = next.snapshotVersion
  const b = stored.snapshotVersion
  if (!a || !b) return false
  if (a.producerId !== b.producerId) return true
  const attempt = BigInt(a.attemptRevision) - BigInt(b.attemptRevision)
  return attempt < 0n || (attempt === 0n && BigInt(a.snapshotRevision) < BigInt(b.snapshotRevision))
}

/** Content locators own row order; current history indices only resolve authorized metadata. */
export function receiveFileRows(
  snapshot: ReceiveSnapshot
): Array<{ index: number; item: ReceiveItem }> {
  return snapshot.items
    .flatMap(item => (item.content?.kind === 'file' ? [{ index: item.content.index, item }] : []))
    .sort((a, b) => a.index - b.index)
}

export function receiveFileContent(
  snapshot: ReceiveSnapshot,
  history: ReceiveHistoryFiles | null,
  originalNames: string[],
  unnamed: (index: number) => string
): ClipboardFileItem {
  const rows = receiveFileRows(snapshot)
  const originalComplete =
    originalNames.length === rows.length && rows.every((row, i) => row.index === i)
  const metadata = rows.map(row =>
    row.item.historyFileIndex != null ? history?.[row.item.historyFileIndex] : undefined
  )
  return {
    file_names: rows.map(
      (row, i) =>
        metadata[i]?.name ??
        (originalComplete ? originalNames[row.index] : undefined) ??
        unnamed(row.index + 1)
    ),
    file_sizes: rows.map(row =>
      row.item.transfer?.totalBytes != null ? Number(row.item.transfer.totalBytes) : -1
    ),
    file_paths: metadata.map(file => file?.path ?? null),
    file_missing: metadata.map(file => file?.missing ?? true),
  }
}

/** Keep exact counter arithmetic outside the platform presentation. */
export function receiveTransferMetrics(item: ReceiveItem) {
  const received = item.transfer ? BigInt(item.transfer.receivedBytes) : null
  const total = item.transfer?.totalBytes != null ? BigInt(item.transfer.totalBytes) : null
  const percent =
    received !== null && total !== null && total > 0n
      ? Math.min(100, Number((received * 10000n) / total) / 100)
      : undefined
  const active = item.state === 'pending' || item.state === 'transferring'
  return {
    received,
    total,
    percent,
    showBar: percent !== undefined || (active && item.transfer != null),
  }
}
