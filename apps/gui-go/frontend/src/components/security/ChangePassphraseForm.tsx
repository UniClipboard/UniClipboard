import ChangePassphraseFields from './ChangePassphraseFields'
import { useChangePassphraseForm } from './useChangePassphraseForm'

export default function ChangePassphraseForm({
  onCancel,
  ...props
}: {
  submitting: boolean
  onSubmittingChange: (submitting: boolean) => void
  onCancel: () => void
  onChanged: () => void
}) {
  const model = useChangePassphraseForm(props)
  return <ChangePassphraseFields model={model} onCancel={onCancel} />
}
