import { Check, X, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { PairingConfirmStepProps } from './types'
import { Button } from '@/components/ui/button'
import StepLayout from '@/pages/setup/StepLayout'

export default function PairingConfirmStep({
  shortCode,
  peerFingerprint,
  onConfirm,
  onCancel,
  error,
  loading,
  direction,
}: PairingConfirmStepProps) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.pairingConfirm' })

  const resolvedError =
    error === 'PairingRejected' ? t('errors.rejected') : error ? t('errors.generic') : null

  const footerButtons = (
    <div className="flex gap-4">
      <Button variant="outline" onClick={onCancel} disabled={loading}>
        <X className="mr-2 h-4 w-4" />
        {t('actions.cancel')}
      </Button>
      <Button onClick={onConfirm} disabled={loading}>
        {loading ? (
          <>
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            {t('actions.confirming')}
          </>
        ) : (
          <>
            <Check className="mr-2 h-4 w-4" />
            {t('actions.confirm')}
          </>
        )}
      </Button>
    </div>
  )

  return (
    <StepLayout
      variant="centered"
      title={t('title')}
      subtitle={t('subtitle')}
      error={resolvedError}
      footer={footerButtons}
      direction={direction}
    >
      <div className="mb-6 text-center sm:mb-8">
        <div className="text-4xl font-mono font-semibold tracking-widest text-primary sm:text-5xl">
          {shortCode}
        </div>
        {peerFingerprint && (
          <div className="mt-6 border-t border-border/30 pt-6">
            <div className="mb-1 text-xs text-muted-foreground">{t('peerFingerprint')}</div>
            <div className="break-all font-mono text-xs opacity-70">{peerFingerprint}</div>
          </div>
        )}
      </div>
    </StepLayout>
  )
}
