import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { openImageExternally, saveImageAs } from '@/api/storage'
import { toast } from '@/components/ui/toast'
import { useShortcut } from '@/hooks/useShortcut'
import { imageFileName, imageFormatLabel, loadImageBlob } from '@/lib/image-handoff'
import { createLogger } from '@/lib/logger'

const log = createLogger('history-image-actions')

export type ImageViewMode = 'fit' | 'actual'

// Keys that act on the focused control must keep working with Space.
function spaceBelongsToFocusedControl(): boolean {
  const el = document.activeElement
  return (
    el instanceof HTMLElement &&
    (el.isContentEditable ||
      el.closest('button, a, input, textarea, select, [role="button"]') !== null)
  )
}

/**
 * State and actions for the image detail: Fit / 100%, Quick Look (Space), the
 * image's format, and handing its bytes to Save as… or the default viewer.
 * Everything resets when `entryId` changes.
 */
export function useHistoryImageActions(
  entryId: string | undefined,
  descriptor: string | null,
  enabled: boolean
) {
  const { t } = useTranslation()
  const [mode, setMode] = useState<ImageViewMode>('fit')
  const [quickLook, setQuickLook] = useState(false)
  const [mime, setMime] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  // A different entry starts over; adjusting state during render avoids a stale frame.
  const [prevEntryId, setPrevEntryId] = useState(entryId)
  if (entryId !== prevEntryId) {
    setPrevEntryId(entryId)
    setMode('fit')
    setQuickLook(false)
    setMime(null)
  }

  useShortcut({
    key: 'space',
    scope: 'clipboard',
    enabled: enabled && !quickLook,
    preventDefault: false,
    handler: () => {
      if (!spaceBelongsToFocusedControl()) setQuickLook(true)
    },
  })

  const run = useCallback(
    async (action: (fileName: string, bytes: Uint8Array) => Promise<unknown>, failure: string) => {
      if (!descriptor || busy) return
      setBusy(true)
      try {
        const blob = await loadImageBlob(descriptor)
        const fileName = imageFileName(t('history.detail.imageFileName'), blob.type || mime)
        await action(fileName, new Uint8Array(await blob.arrayBuffer()))
      } catch (err) {
        log.error({ err }, failure)
        toast.error(t('history.detail.imageActionFailed'))
      } finally {
        setBusy(false)
      }
    },
    [busy, descriptor, mime, t]
  )

  return {
    mode,
    setMode,
    quickLook,
    setQuickLook,
    formatLabel: imageFormatLabel(mime),
    onImageType: setMime,
    busy,
    canAct: descriptor !== null,
    saveAs: () => run(saveImageAs, 'failed to save image'),
    openExternally: () => run(openImageExternally, 'failed to open image externally'),
  }
}
