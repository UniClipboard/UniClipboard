import React from 'react'
import type { ClipboardFileItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import ClipboardItemRow from './ClipboardItemRow'
import FileContextMenu from './FileContextMenu'

interface ClipboardListRowProps {
  item: DisplayClipboardItem
  isActive: boolean
  isStale: boolean
  onSelect: (id: string) => void
  onCopy: (id: string) => void
  onDelete: (id: string) => void
  onOpenFileLocation: (id: string) => void
}

/**
 * One virtualized clipboard row: the context-menu wrapper plus the row itself.
 *
 * Memoized so a render of the parent list (selection change, a new item
 * prepended, the shared 30s clock tick) re-renders only the rows whose props
 * actually changed — not the whole list. The callbacks must be referentially
 * stable for memoization to bite, so the parent passes id-taking handlers and
 * this component builds the per-row closures internally (recreated only when
 * this row re-renders). See issue #1129.
 */
function ClipboardListRowImpl({
  item,
  isActive,
  isStale,
  onSelect,
  onCopy,
  onDelete,
  onOpenFileLocation,
}: ClipboardListRowProps) {
  const hasMissingFiles =
    item.type === 'file'
      ? ((item.content as ClipboardFileItem | null)?.file_missing?.some(Boolean) ?? false)
      : false

  return (
    <FileContextMenu
      itemId={item.id}
      itemType={item.type}
      transferStatus={{ isStale, hasMissingFiles }}
      onCopy={onCopy}
      onDelete={onDelete}
      onOpenFileLocation={onOpenFileLocation}
    >
      <ClipboardItemRow
        item={item}
        isActive={isActive}
        isStale={isStale}
        onClick={() => onSelect(item.id)}
      />
    </FileContextMenu>
  )
}

const ClipboardListRow = React.memo(ClipboardListRowImpl)

export default ClipboardListRow
