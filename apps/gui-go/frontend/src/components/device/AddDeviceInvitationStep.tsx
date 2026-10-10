import { Loader2, RefreshCw } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { AddDeviceInvitation } from '@/components/device/AddDeviceInvitation'
import { Button } from '@/components/ui/button'
import type { useAddDeviceInvitation } from '@/hooks/useAddDeviceInvitation'

export default function AddDeviceInvitationStep({
  invitationState,
  hasExternalActions,
}: {
  invitationState: ReturnType<typeof useAddDeviceInvitation>
  hasExternalActions: boolean
}) {
  const { t } = useTranslation()
  const {
    invitation,
    loading,
    error,
    issueRetryable,
    remaining,
    expired,
    progress,
    display,
    handleRegenerate,
  } = invitationState

  if (loading && !invitation) {
    return (
      <div className="flex items-center justify-center gap-3 py-12 text-ui-body text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t('devices.addDevice.loading')}
      </div>
    )
  }
  if (error && !invitation) {
    return (
      <div className="flex flex-col items-center gap-3 py-10">
        <p className="text-ui-body text-destructive">{error}</p>
        {!hasExternalActions && issueRetryable !== false && (
          <Button variant="outline" size="sm" onClick={handleRegenerate} disabled={loading}>
            <RefreshCw className="mr-2 size-3.5" />
            {t('devices.addDevice.actions.regenerate')}
          </Button>
        )}
      </div>
    )
  }
  if (!invitation) return null
  return (
    <AddDeviceInvitation
      invitation={invitation}
      expired={expired}
      display={display}
      progress={progress}
      remaining={remaining}
    />
  )
}
