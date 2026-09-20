import { Settings2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { MembershipMaintenanceHealthDto } from '@/api/generated/types.gen'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'

export function MembershipMaintenanceNotice({
  health,
  onReview,
}: {
  health?: MembershipMaintenanceHealthDto | null
  onReview: () => void
}) {
  const { t } = useTranslation()
  if (!health || health.phase === 'healthy') return null

  return (
    <Alert
      data-testid="membership-maintenance-alert"
      className="mt-2 border-warning/30 bg-warning/10 text-warning"
    >
      <AlertDescription className="flex flex-col gap-2 text-ui-caption">
        <span>
          {t(
            `devices.membershipMaintenance.${health.phase === 'retrying' ? 'retrying' : 'needsAttention'}`
          )}
        </span>
        {health.recovery === 'resolve_device_trust' && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="self-start"
            onClick={onReview}
          >
            <Settings2 />
            {t('devices.membershipMaintenance.review')}
          </Button>
        )}
      </AlertDescription>
    </Alert>
  )
}
