import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { shortInvitationInput } from '@/lib/invitation-code'

/** Secondary invitation input for the platform-less setup views. */
export function FullInvitationInput({
  id,
  value,
  onChange,
  disabled,
}: {
  id: string
  value: string
  onChange: (value: string) => void
  disabled?: boolean
}) {
  const { t } = useTranslation(undefined, { keyPrefix: 'setup.redeemInvitation' })
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{t('labels.fullInvitation')}</Label>
      <Input
        id={id}
        value={shortInvitationInput(value) === value ? '' : value}
        onChange={event => onChange(event.target.value)}
        disabled={disabled}
        placeholder="ucspace1_…"
        autoComplete="off"
        aria-describedby={`${id}-hint`}
      />
      <p id={`${id}-hint`} className="text-ui-caption text-muted-foreground">
        {t('fullInvitationHint')}
      </p>
    </div>
  )
}
