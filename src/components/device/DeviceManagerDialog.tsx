import { useTranslation } from 'react-i18next'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import DevicesPage from '@/pages/DevicesPage'

interface DeviceManagerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

function DeviceManagerDialog({ open, onOpenChange }: DeviceManagerDialogProps) {
  const { t } = useTranslation()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="h-[calc(100%-4rem)] w-[calc(100%-4rem)] max-w-none gap-0 overflow-hidden bg-card p-0 text-card-foreground sm:max-w-none">
        <DialogTitle className="sr-only">{t('nav.devices')}</DialogTitle>
        <DialogDescription className="sr-only">{t('devices.panel.listTitle')}</DialogDescription>
        <DevicesPage />
      </DialogContent>
    </Dialog>
  )
}

export default DeviceManagerDialog
