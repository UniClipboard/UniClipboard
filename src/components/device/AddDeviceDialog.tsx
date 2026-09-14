import AddDeviceDialogSession, {
  type AddDeviceDialogProps,
} from '@/components/device/AddDeviceDialogSession'
import { useDialogSessionReset } from '@/hooks/useDialogSessionReset'

export default function AddDeviceDialog(props: AddDeviceDialogProps) {
  const { sessionKey, onOpenChangeComplete } = useDialogSessionReset()
  return (
    <AddDeviceDialogSession
      key={sessionKey}
      {...props}
      onOpenChangeComplete={onOpenChangeComplete}
    />
  )
}
