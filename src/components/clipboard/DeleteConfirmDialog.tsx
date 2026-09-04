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
import { Checkbox } from '@/components/ui/checkbox'
import { Kbd } from '@/components/ui/kbd'
import { useShortcut } from '@/hooks/useShortcut'
import { useShortcutLayer } from '@/hooks/useShortcutLayer'

interface DeleteConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: (skipFutureConfirmation: boolean) => void | Promise<void>
  count: number
}

const DeleteConfirmDialog: React.FC<DeleteConfirmDialogProps> = ({
  open,
  onOpenChange,
  onConfirm,
  count,
}) => {
  const { t } = useTranslation()
  const [isDeleting, setIsDeleting] = useState(false)
  const [skipFutureConfirmation, setSkipFutureConfirmation] = useState(false)

  // 当对话框打开时，激活 modal 作用域
  useShortcutLayer({
    layer: 'modal',
    scope: 'modal',
    enabled: open,
  })

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) setSkipFutureConfirmation(false)
    onOpenChange(nextOpen)
  }

  const handleConfirm = async () => {
    setIsDeleting(true)
    try {
      await onConfirm(skipFutureConfirmation)
      handleOpenChange(false)
    } finally {
      setIsDeleting(false)
    }
  }

  const handleCancel = () => {
    if (!isDeleting) {
      handleOpenChange(false)
    }
  }

  // ESC 取消
  useShortcut({
    key: 'esc',
    scope: 'modal',
    enabled: open && !isDeleting,
    handler: handleCancel,
  })

  // D 或 Y 确认删除
  useShortcut({
    key: 'd',
    scope: 'modal',
    enabled: open && !isDeleting,
    handler: () => void handleConfirm(),
  })

  useShortcut({
    key: 'y',
    scope: 'modal',
    enabled: open && !isDeleting,
    handler: () => void handleConfirm(),
  })

  return (
    <AlertDialog open={open} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('clipboard.confirmDeleteTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('clipboard.confirmDeleteDescription', { count })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <label className="flex cursor-pointer items-center gap-2 text-sm text-muted-foreground">
          <Checkbox
            checked={skipFutureConfirmation}
            onCheckedChange={setSkipFutureConfirmation}
            disabled={isDeleting}
          />
          <span>{t('clipboard.skipDeleteConfirmationLabel')}</span>
        </label>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isDeleting} onClick={handleCancel}>
            <span className="flex items-center gap-1.5">
              {t('clipboard.cancelLabel')}
              <Kbd>ESC</Kbd>
            </span>
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="history-delete-confirm"
            onClick={e => {
              e.preventDefault()
              handleConfirm()
            }}
            disabled={isDeleting}
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
          >
            {isDeleting ? (
              t('clipboard.deletingLabel')
            ) : (
              <span className="flex items-center gap-1.5">
                {t('clipboard.deleteLabel')}
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

export default DeleteConfirmDialog
