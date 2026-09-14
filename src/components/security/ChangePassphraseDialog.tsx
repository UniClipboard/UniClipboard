import { useTranslation } from 'react-i18next'
import ChangePassphraseForm from '@/components/security/ChangePassphraseForm'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

export default function ChangePassphraseDialog({
  open,
  onOpenChange,
  onChanged,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onChanged: () => void
}) {
  const { t } = useTranslation()

  return (
    <Dialog open={open} onOpenChange={onOpenChange} disablePointerDismissal>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('passphraseChange.title')}</DialogTitle>
          <DialogDescription>{t('passphraseChange.description')}</DialogDescription>
        </DialogHeader>
        <ChangePassphraseForm
          key={open ? 'open' : 'closed'}
          onCancel={() => onOpenChange(false)}
          onChanged={onChanged}
        />
      </DialogContent>
    </Dialog>
  )
}
