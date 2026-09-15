import { useState } from 'react'
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
  const [submitting, setSubmitting] = useState(false)
  const handleOpenChange = (nextOpen: boolean) => {
    if (!submitting) onOpenChange(nextOpen)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange} disablePointerDismissal>
      <DialogContent showCloseButton={!submitting}>
        <DialogHeader>
          <DialogTitle>{t('passphraseChange.title')}</DialogTitle>
          <DialogDescription>{t('passphraseChange.description')}</DialogDescription>
        </DialogHeader>
        <ChangePassphraseForm
          key={open ? 'open' : 'closed'}
          submitting={submitting}
          onSubmittingChange={setSubmitting}
          onCancel={() => handleOpenChange(false)}
          onChanged={onChanged}
        />
      </DialogContent>
    </Dialog>
  )
}
