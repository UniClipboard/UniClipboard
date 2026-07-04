import { Check, Copy } from 'lucide-react'
import React from 'react'
import { useTranslation } from 'react-i18next'
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard'

/**
 * Small inline copy-to-clipboard button with a transient "copied" check.
 * Shared by the device detail panels (peer id rows and similar).
 */
const CopyIconButton: React.FC<{ value: string; label?: string }> = ({ value, label }) => {
  const { t } = useTranslation()
  const { copied, copy } = useCopyToClipboard()
  const title = copied
    ? t('devices.panel.profile.copied')
    : (label ?? t('devices.panel.profile.copy'))
  return (
    <button
      type="button"
      aria-label={title}
      title={title}
      onClick={() => void copy(value)}
      className="rounded-md p-1 text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
    >
      {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3" />}
    </button>
  )
}

export default CopyIconButton
