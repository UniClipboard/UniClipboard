import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { useCopyFullInvitation } from '@/hooks/useCopyFullInvitation'

export function LegacyCopyFullInvitationButton({
  invitation,
  disabled,
}: {
  invitation: string
  disabled?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.showInvitation' })
  const { status, copy } = useCopyFullInvitation(invitation)
  return (
    <div className="flex flex-col gap-2">
      <Button type="button" variant="outline" disabled={disabled} onClick={() => void copy()}>
        {t(status === 'copied' ? 'actions.fullInvitationCopied' : 'actions.copyFullInvitation')}
      </Button>
      <p className="text-ui-caption text-muted-foreground">{t('fullInvitationHint')}</p>
      {status === 'failed' && (
        <p role="alert" className="text-ui-body text-destructive">
          {t('copyError')}
        </p>
      )}
    </div>
  )
}
