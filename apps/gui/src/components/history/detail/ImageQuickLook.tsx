import { X } from 'lucide-react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { useShortcut } from '@/hooks/useShortcut'

interface ImageQuickLookProps {
  /** Resolved image URL; the overlay renders nothing until there is one. */
  src: string | null
  onClose: () => void
}

/** Full-window image preview opened with Space, like Finder's Quick Look. */
export default function ImageQuickLook({ src, onClose }: ImageQuickLookProps) {
  const { t } = useTranslation()
  useShortcut({ key: 'escape', scope: 'clipboard', handler: onClose, capture: true })

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t('history.detail.quickLook')}
      data-testid="image-quick-look"
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-8 backdrop-blur-sm"
    >
      {src && (
        <img
          src={src}
          alt={t('clipboard.item.altText.clipboardImage')}
          onClick={event => event.stopPropagation()}
          className="max-h-full max-w-full rounded-lg object-contain shadow-2xl"
        />
      )}
      <button
        type="button"
        aria-label={t('history.detail.quickLookClose')}
        onClick={onClose}
        className="absolute right-5 top-5 flex size-9 items-center justify-center rounded-full bg-black/60 text-white transition-colors hover:bg-black/80"
      >
        <X className="size-4" aria-hidden="true" />
      </button>
    </div>,
    document.body
  )
}
