import { Loader2 } from 'lucide-react'
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
  busy: boolean
  onConfirm: () => void
}

const UnpairAlertDialog: React.FC<UnpairAlertDialogProps> = ({
  open,
  onOpenChange,
  deviceName,
  busy,
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
          <AlertDialogCancel disabled={busy}>{t('clipboard.cancelLabel')}</AlertDialogCancel>
          <AlertDialogAction
            data-testid="device-unpair-confirm"
            variant="destructive"
            disabled={busy}
            aria-busy={busy}
            onClick={onConfirm}
          >
            {busy && <Loader2 aria-hidden="true" className="size-4 animate-spin" />}
            {busy ? t('devices.unpair.cancelling') : t('devices.list.actions.unpair')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

export default UnpairAlertDialog
