import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Kbd } from '@/components/ui/kbd'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'
import { SettingAlert } from './SettingControls'

interface ClearHistoryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void | Promise<void>
}

const ClearHistoryDialog: React.FC<ClearHistoryDialogProps> = ({
  open,
  onOpenChange,
  onConfirm,
}) => {
  const { t } = useTranslation()
  const [isClearing, setIsClearing] = useState(false)

  // Activate modal scope when dialog opens
  useShortcutLayer({
    layer: 'modal',
    scope: 'modal',
    enabled: open,
  })

  const handleConfirm = async () => {
    setIsClearing(true)
    try {
      await onConfirm()
      onOpenChange(false)
    } finally {
      setIsClearing(false)
    }
  }

  const handleCancel = () => {
    if (!isClearing) {
      onOpenChange(false)
    }
  }

  // ESC to cancel
  useShortcut({
    key: 'esc',
    scope: 'modal',
    enabled: open && !isClearing,
    handler: handleCancel,
  })

  // D or Y to confirm
  useShortcut({
    key: 'd',
    scope: 'modal',
    enabled: open && !isClearing,
    handler: () => void handleConfirm(),
  })

  useShortcut({
    key: 'y',
    scope: 'modal',
    enabled: open && !isClearing,
    handler: () => void handleConfirm(),
  })

  return (
    <SettingAlert
      open={open}
      onOpenChange={onOpenChange}
      title={t('settings.sections.storage.clearHistory.dialogTitle')}
      description={t('settings.sections.storage.clearHistory.dialogDescription')}
      confirmLabel={t('settings.sections.storage.clearHistory.dialogConfirm')}
      cancelLabel={t('clipboard.cancelLabel')}
      destructive
      busyLabel={isClearing ? t('settings.sections.storage.clearHistory.clearing') : undefined}
      cancelHint={<Kbd>ESC</Kbd>}
      confirmHint={
        <span className="flex items-center gap-0.5">
          <Kbd>D</Kbd>
          <span className="text-ui-caption text-destructive-foreground/70 mx-0.5">/</span>
          <Kbd>Y</Kbd>
        </span>
      }
      onConfirm={handleConfirm}
    />
  )
}

export default ClearHistoryDialog
