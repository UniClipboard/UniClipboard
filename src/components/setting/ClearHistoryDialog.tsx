import React, { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog'
import { Kbd } from '@/components/ui/kbd'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'

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
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {t('settings.sections.storage.clearHistory.dialogTitle')}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {t('settings.sections.storage.clearHistory.dialogDescription')}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isClearing} onClick={handleCancel}>
            <span className="flex items-center gap-1.5">
              {t('clipboard.cancelLabel')}
              <Kbd>ESC</Kbd>
            </span>
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={e => {
              e.preventDefault()
              handleConfirm()
            }}
            disabled={isClearing}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {isClearing ? (
              t('settings.sections.storage.clearHistory.clearing')
            ) : (
              <span className="flex items-center gap-1.5">
                {t('settings.sections.storage.clearHistory.dialogConfirm')}
                <span className="flex items-center gap-0.5">
                  <Kbd>D</Kbd>
                  <span className="text-xs text-destructive-foreground/70 mx-0.5">/</span>
                  <Kbd>Y</Kbd>
                </span>
              </span>
            )}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default ClearHistoryDialog
