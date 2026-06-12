import { useEffect, useState } from 'react'
import type { ClipboardTextItem, DisplayClipboardItem } from '@/lib/clipboard-entry'
import { clipboardPreviewCache, type ClipboardPreviewData } from '@/lib/clipboard-preview-cache'
import { createLogger } from '@/lib/logger'
import { useAppSelector } from '@/store/hooks'
import {
  type EntryTransferStatus,
  resolveEntryTransferStatus,
  selectEntryTransferStatus,
  selectTransferByEntryId,
  type TransferProgressInfo,
} from '@/store/slices/fileTransferSlice'

const log = createLogger('clipboard-preview')

export interface ClipboardPreviewState {
  effectiveStatus: ReturnType<typeof resolveEntryTransferStatus>
  entryStatus: EntryTransferStatus | undefined
  imageDimensions: { width: number; height: number } | null
  loading: boolean
  preview: ClipboardPreviewData | null
  setImageDimensions: (dims: { width: number; height: number } | null) => void
  transfer: TransferProgressInfo | undefined
}

export function useClipboardPreviewState(item: DisplayClipboardItem | null): ClipboardPreviewState {
  const transfer = useAppSelector(state =>
    item ? selectTransferByEntryId(state, item.id) : undefined
  )
  const entryStatus = useAppSelector(state =>
    item ? selectEntryTransferStatus(state, item.id) : undefined
  )
  const effectiveStatus = resolveEntryTransferStatus(entryStatus, transfer)
  const [preview, setPreview] = useState<ClipboardPreviewData | null>(null)
  const [loading, setLoading] = useState(false)
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

  useEffect(() => {
    setPreview(null)
    setImageDimensions(null)
    setLoading(false)

    if (!item) return

    const shouldLoadPreview =
      item.type === 'image' ||
      item.type === 'file' ||
      item.type === 'code' ||
      (item.type === 'text' && (item.content as ClipboardTextItem).has_detail)

    if (!shouldLoadPreview) return

    let cancelled = false
    setLoading(true)

    void (async () => {
      try {
        const nextPreview = await clipboardPreviewCache.get(item.id)
        if (!cancelled) setPreview(nextPreview)
      } catch (e) {
        if (!cancelled) log.error({ err: e }, 'Failed to load clipboard preview')
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [item])

  return {
    effectiveStatus,
    entryStatus,
    imageDimensions,
    loading,
    preview,
    setImageDimensions,
    transfer,
  }
}
