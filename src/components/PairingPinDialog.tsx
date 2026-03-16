import { ShieldCheck } from 'lucide-react'
import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'

type PinVerifyStep = 'display' | 'verifying' | 'success'

interface PairingPinDialogProps {
  open: boolean
  onClose: () => void
  pinCode: string
  peerDeviceName?: string
  isInitiator: boolean
  onConfirm: (matches: boolean) => void
  phase?: PinVerifyStep
}

/**
 * PIN verification dialog for P2P pairing
 *
 * Used by both:
 * - Initiator: After receiving PIN from peer
 * - Responder: After generating PIN for peer
 */
export default function PairingPinDialog({
  open,
  onClose,
  pinCode,
  peerDeviceName,
  isInitiator,
  onConfirm,
  phase,
}: PairingPinDialogProps) {
  const { t } = useTranslation()
  const [step, setStep] = useState<PinVerifyStep>('display')

  useEffect(() => {
    if (open) {
      setStep(phase ?? 'display')
    }
  }, [open, phase])

  const handleConfirm = (matches: boolean) => {
    setStep('verifying')
    onConfirm(matches)
  }

  const getTitle = () => {
    if (isInitiator) {
      return step === 'display'
        ? t('pairing.pinVerify.initiatorTitle')
        : t('pairing.pinVerify.verifying')
    } else {
      return step === 'display'
        ? t('pairing.pinVerify.responderTitle')
        : t('pairing.pinVerify.verifying')
    }
  }

  const getDescription = () => {
    if (step === 'verifying') {
      return t('pairing.pinVerify.pleaseWait')
    }
    if (isInitiator) {
      return t('pairing.pinVerify.initiatorDescription', { deviceName: peerDeviceName })
    }
    return t('pairing.pinVerify.responderDescription', { deviceName: peerDeviceName })
  }

  const title = step === 'success' ? t('pairing.success.title') : getTitle()
  const description = step === 'success' ? '' : getDescription()

  return (
    <Dialog open={open} onOpenChange={open => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>

        {step === 'display' && (
          <div className="flex flex-col items-center justify-center space-y-6 py-8">
            <div className="p-4 bg-muted/50 rounded-xl border-2 border-dashed border-primary/20 text-center w-full max-w-[240px]">
              <p className="text-sm text-muted-foreground mb-2">
                {t('pairing.pinVerify.pinLabel')}
              </p>
              <div className="text-4xl font-mono font-bold tracking-wider text-primary">
                {pinCode}
              </div>
            </div>

            <div className="flex items-center gap-2 text-sm text-muted-foreground bg-amber-500/10 text-amber-600 px-4 py-2 rounded-full">
              <ShieldCheck className="w-4 h-4" />
              {t('pairing.pinVerify.warning')}
            </div>

            {isInitiator ? (
              <div className="flex gap-4 w-full">
                <Button variant="outline" className="flex-1" onClick={() => handleConfirm(false)}>
                  {t('pairing.pinVerify.notMatch')}
                </Button>
                <Button className="flex-1" onClick={() => handleConfirm(true)}>
                  {t('pairing.pinVerify.match')}
                </Button>
              </div>
            ) : (
              <div className="w-full">
                <Button variant="outline" className="w-full" onClick={() => handleConfirm(false)}>
                  {t('clipboard.cancelLabel')}
                </Button>
              </div>
            )}
          </div>
        )}

        {step === 'verifying' && (
          <div className="flex flex-col items-center justify-center py-12">
            <div className="w-16 h-16 rounded-full bg-primary/10 flex items-center justify-center">
              <ShieldCheck className="w-8 h-8 text-primary animate-pulse" />
            </div>
            <p className="mt-4 text-sm text-muted-foreground">{t('pairing.pinVerify.verifying')}</p>
          </div>
        )}

        {step === 'success' && (
          <div className="flex flex-col items-center justify-center py-12 text-green-600">
            <div className="w-16 h-16 rounded-full bg-green-100 flex items-center justify-center">
              <ShieldCheck className="w-8 h-8" />
            </div>
            <h3 className="mt-4 text-lg font-medium">{t('pairing.success.title')}</h3>
          </div>
        )}
      </DialogContent>
    </Dialog>
  )
}
