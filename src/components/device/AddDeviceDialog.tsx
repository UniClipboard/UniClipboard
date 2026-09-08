import { Check, Copy, Loader2, RefreshCw, XCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { AddDeviceDialogBody } from '@/components/device/AddDeviceDialogBody'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'
import { useDialogSessionReset } from '@/hooks/useDialogSessionReset'

interface AddDeviceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSuccess?: () => void
}

function AddDeviceDialogInner({
  open,
  onOpenChange,
  onSuccess,
  onOpenChangeComplete,
}: AddDeviceDialogProps & { onOpenChangeComplete: (open: boolean) => void }) {
  const { t } = useTranslation()
  const invitationState = useAddDeviceInvitation({ open, onOpenChange, onSuccess })
  const { invitation, loading, step, copied, expired, handleCopy, handleCancel, handleRegenerate } =
    invitationState

  // ── Footer ───────────────────────────────────────────
  let footer: React.ReactNode
  if (step === 'credentials' || step === 'success') {
    // 自动关闭，无按钮
    footer = null
  } else if (step === 'failed' || expired) {
    footer = (
      <>
        <Button variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
          {t('devices.addDevice.actions.close')}
        </Button>
        <Button onClick={handleRegenerate} disabled={loading}>
          {loading ? (
            <Loader2 className="mr-2 size-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 size-4" />
          )}
          {t('devices.addDevice.actions.regenerate')}
        </Button>
      </>
    )
  } else {
    footer = (
      <>
        <Button variant="ghost" onClick={handleCancel} disabled={loading || !invitation}>
          {loading ? (
            <Loader2 className="mr-2 size-4 animate-spin" />
          ) : (
            <XCircle className="mr-2 size-4" />
          )}
          {t('devices.addDevice.actions.cancel')}
        </Button>
        <Button
          variant={copied ? 'outline' : 'default'}
          onClick={handleCopy}
          disabled={!invitation || loading}
        >
          {copied ? (
            <>
              <Check className="mr-2 size-4" />
              {t('devices.addDevice.actions.copied')}
            </>
          ) : (
            <>
              <Copy className="mr-2 size-4" />
              {t('devices.addDevice.actions.copy')}
            </>
          )}
        </Button>
      </>
    )
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      onOpenChangeComplete={onOpenChangeComplete}
      disablePointerDismissal
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {step === 'credentials'
              ? t('devices.addDevice.rePairing.title')
              : step === 'success'
                ? t('devices.addDevice.success.title')
                : step === 'failed'
                  ? t('devices.addDevice.failed.title')
                  : t('devices.addDevice.title')}
          </DialogTitle>
          {(step === 'credentials' || step === 'invitation') && (
            <DialogDescription>
              {step === 'credentials'
                ? t('devices.addDevice.rePairing.subtitle')
                : t('devices.addDevice.subtitle')}
            </DialogDescription>
          )}
        </DialogHeader>

        <AddDeviceDialogBody invitationState={invitationState} />

        {footer && <DialogFooter>{footer}</DialogFooter>}
      </DialogContent>
    </Dialog>
  )
}

export default function AddDeviceDialog(props: AddDeviceDialogProps) {
  const { sessionKey, onOpenChangeComplete } = useDialogSessionReset()
  return (
    <AddDeviceDialogInner key={sessionKey} {...props} onOpenChangeComplete={onOpenChangeComplete} />
  )
}
