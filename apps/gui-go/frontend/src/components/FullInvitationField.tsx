import { useTranslation } from 'react-i18next'
import { shortInvitationInput } from '@/lib/invitation-code'
import { FormField } from '@/ui-kit'

/** Secondary entry for an opaque invitation when local discovery is unavailable. */
export function FullInvitationField({
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
    <FormField
      id={id}
      label={t('labels.fullInvitation')}
      hint={t('fullInvitationHint')}
      placeholder="ucspace1_…"
      value={shortInvitationInput(value) === value ? '' : value}
      onValueChange={onChange}
      disabled={disabled}
      autoComplete="off"
    />
  )
}
