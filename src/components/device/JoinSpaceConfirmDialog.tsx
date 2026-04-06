import React from 'react'
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

interface JoinSpaceConfirmDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onConfirm: () => void
}

const JoinSpaceConfirmDialog: React.FC<JoinSpaceConfirmDialogProps> = ({
  open,
  onOpenChange,
  onConfirm,
}) => {
  const { t } = useTranslation()

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('devices.list.actions.joinSpaceConfirm.title')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('devices.list.actions.joinSpaceConfirm.description')}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('devices.list.actions.joinSpaceConfirm.cancel')}</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>
            {t('devices.list.actions.joinSpaceConfirm.confirm')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default JoinSpaceConfirmDialog
