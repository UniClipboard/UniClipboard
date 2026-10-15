import { useTranslation } from 'react-i18next'
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard'
import { Button } from '@/ui-kit'

/** An icon button that copies `value` and names the copy for a moment. */
export function DevicesKitCopyButton({ value, label }: { value: string; label?: string }) {
  const { t } = useTranslation()
  const { copied, copy } = useCopyToClipboard()
  return (
    <Button
      variant="subtle"
      size="small"
      icon="copy"
      label={
        copied ? t('devices.panel.profile.copied') : (label ?? t('devices.panel.profile.copy'))
      }
      onClick={() => void copy(value)}
    />
  )
}
