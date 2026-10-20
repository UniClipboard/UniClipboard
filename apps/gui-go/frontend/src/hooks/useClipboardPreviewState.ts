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
  const entryId = item?.id ?? null
  const textItem =
    item?.type === 'text' || item?.type === 'richtext'
      ? (item.content as ClipboardTextItem | null)
      : null
  const shouldLoadPreview =
    item?.type === 'image' ||
    item?.type === 'file' ||
    (textItem !== null && (item?.contentTags?.includes('code') === true || textItem.has_detail))
  const [request, setRequest] = useState(() => {
    const preview = entryId && shouldLoadPreview ? clipboardPreviewCache.peek(entryId) : null
    return { entryId, preview, loading: shouldLoadPreview && !preview }
  })
  const [imageDimensions, setImageDimensions] = useState<{ width: number; height: number } | null>(
    null
  )

  // Reset selection state before paint; object replacements for the same entry keep their data.
  if (request.entryId !== entryId) {
    const preview = entryId && shouldLoadPreview ? clipboardPreviewCache.peek(entryId) : null
    setRequest({ entryId, preview, loading: shouldLoadPreview && !preview })
    setImageDimensions(null)
  }

  useEffect(() => {
    if (!entryId || !shouldLoadPreview) return

    let cancelled = false

    void (async () => {
      try {
        const preview = await clipboardPreviewCache.get(entryId)
        if (!cancelled) setRequest({ entryId, preview, loading: false })
      } catch (e) {
        if (!cancelled) log.error({ err: e }, 'Failed to load clipboard preview')
      } finally {
        if (!cancelled) setRequest(current => ({ ...current, loading: false }))
      }
    })()

    return () => {
      cancelled = true
    }
  }, [entryId, shouldLoadPreview])

  return {
    effectiveStatus,
    entryStatus,
    imageDimensions,
    loading: request.loading,
    preview: request.preview,
    setImageDimensions,
    transfer,
  }
}
