import { useTranslation } from 'react-i18next'
import { useCopyFullInvitation } from '@/hooks/useCopyFullInvitation'
import { Button, InlineNotice, Text } from '@/ui-kit'

/** Copy admission material without passing it to telemetry, logs, or storage. */
export function CopyFullInvitationButton({
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
      <Button icon="copy" disabled={disabled} onClick={() => void copy()}>
        {t(status === 'copied' ? 'actions.fullInvitationCopied' : 'actions.copyFullInvitation')}
      </Button>
      <Text role="caption" tone="secondary">
        {t('fullInvitationHint')}
      </Text>
      {status === 'failed' && <InlineNotice tone="error" title={t('copyError')} />}
    </div>
  )
}
