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

interface UnpairAlertDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceName: string
  onConfirm: () => void
}

const UnpairAlertDialog: React.FC<UnpairAlertDialogProps> = ({
  open,
  onOpenChange,
  deviceName,
  onConfirm,
}) => {
  const { t } = useTranslation()

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('devices.unpair.confirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('devices.unpair.confirmDescription', { deviceName })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('clipboard.cancelLabel')}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={onConfirm}>
            {t('devices.list.actions.unpair')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default UnpairAlertDialog
