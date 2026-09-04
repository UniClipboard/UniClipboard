import { MonitorCog } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui'

interface RePairingNoticeProps {
  onOpenDevices: () => void
}

export default function RePairingNotice({ onOpenDevices }: RePairingNoticeProps) {
  const { t } = useTranslation()

  return (
    <AlertDialog open>
      <AlertDialogContent className="bg-card text-card-foreground" style={{ animation: 'none' }}>
        <AlertDialogHeader>
          <div className="mb-2 flex size-10 items-center justify-center rounded-lg bg-warning/15 text-warning">
            <MonitorCog className="size-5" />
          </div>
          <AlertDialogTitle>{t('rePairingNotice.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('rePairingNotice.body')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogAction onClick={onOpenDevices}>
            {t('rePairingNotice.goToDevices')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
