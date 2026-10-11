import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { AlertDialog } from '@/ui-kit'

/**
 * Confirmation before deleting entries. Deletion is permanent (the Engine keeps no trash), so the
 * alert is destructive: keyboard focus starts on Cancel. "Do not ask again" keeps the existing
 * preference that turns the confirmation off.
 */
export function HistoryKitDeleteAlert({
  open,
  onOpenChange,
  count,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  count: number
  onConfirm: (skipFutureConfirmation: boolean) => void
}) {
  const { t } = useTranslation()
  const [skip, setSkip] = useState(false)
  // Page shortcuts (C, D, T, ...) stay off while the alert is up.
  useShortcutLayer({ layer: 'modal', scope: 'modal', enabled: open })
  const change = (next: boolean) => {
    if (!next) setSkip(false)
    onOpenChange(next)
  }
  return (
    <AlertDialog
      open={open}
      onOpenChange={change}
      title={t('clipboard.confirmDeleteTitle')}
      description={t('clipboard.confirmDeleteDescription', { count })}
      confirmLabel={t('clipboard.deleteLabel')}
      cancelLabel={t('clipboard.cancelLabel')}
      destructive
      onConfirm={() => onConfirm(skip)}
      suppression={{
        label: t('clipboard.skipDeleteConfirmationLabel'),
        checked: skip,
        onCheckedChange: setSkip,
      }}
    />
  )
}
