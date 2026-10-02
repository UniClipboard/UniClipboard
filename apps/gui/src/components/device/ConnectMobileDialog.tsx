import ConnectMobileDialogSession, {
  type ConnectMobileDialogProps,
} from '@/components/device/ConnectMobileDialogSession'
import { useDialogSessionReset } from '@/hooks/useDialogSessionReset'

export default function ConnectMobileDialog(props: ConnectMobileDialogProps) {
  const { sessionKey, onOpenChangeComplete } = useDialogSessionReset()
  return (
    <ConnectMobileDialogSession
      key={sessionKey}
      {...props}
      onOpenChangeComplete={onOpenChangeComplete}
    />
  )
}
