import { Check, Copy, Loader2, RefreshCw, XCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export default function DeviceInvitationActions({
  invitationState,
  onOpenChange,
  formId,
}: {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  onOpenChange: (open: boolean) => void
  formId?: string
}) {
  const { t } = useTranslation()
  const { invitation, loading, step, copied, expired, handleCopy, handleCancel, handleRegenerate } =
    invitationState
  if (step === 'success') return null
  if (step === 'credentials') {
    return formId ? (
      <Button
        type="submit"
        form={formId}
        data-testid="re-pairing-confirm-passphrase"
        disabled={loading || !invitationState.passphrase.trim()}
      >
        {loading && <Loader2 className="size-4 animate-spin" />}
        {t(
          loading ? 'devices.addDevice.rePairing.submitting' : 'devices.addDevice.rePairing.submit'
        )}
      </Button>
    ) : null
  }
  const retry = step === 'failed' || expired || (!!formId && !invitation && !!invitationState.error)
  return (
    <>
      {retry ? (
        <>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={loading}>
            {t('devices.addDevice.actions.close')}
          </Button>
          <Button onClick={handleRegenerate} disabled={loading}>
            {loading ? (
              <Loader2 className="size-4 animate-spin" />
            ) : (
              <RefreshCw className="size-4" />
            )}
            {t('devices.addDevice.actions.regenerate')}
          </Button>
        </>
      ) : (
        <>
          <Button variant="ghost" onClick={handleCancel} disabled={loading || !invitation}>
            {loading ? <Loader2 className="size-4 animate-spin" /> : <XCircle className="size-4" />}
            {t('devices.addDevice.actions.cancel')}
          </Button>
          <Button
            variant={copied ? 'outline' : 'default'}
            onClick={handleCopy}
            disabled={!invitation || loading}
          >
            {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
            {t(copied ? 'devices.addDevice.actions.copied' : 'devices.addDevice.actions.copy')}
          </Button>
        </>
      )}
    </>
  )
}
