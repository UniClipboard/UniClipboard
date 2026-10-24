import { Check, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { normalizeCancelReason, type EntryTransferStatus } from '@/store/slices/fileTransferSlice'

const CANCEL_REASONS = new Set(['local_user', 'remote_peer', 'replaced', 'timeout', 'unknown'])

export default function FilePreviewStatus({
  status,
  reason,
}: {
  status: EntryTransferStatus['status'] | undefined
  reason?: string | null
}) {
  const { t } = useTranslation()
  if (!status) return null
  const normalized = normalizeCancelReason(reason)
  const cancelledReason = normalized && CANCEL_REASONS.has(normalized) ? normalized : 'unknown'
  return (
    <span
      role="status"
      className={`flex items-center gap-1.5 text-ui-caption ${status === 'failed' ? 'text-destructive' : 'text-muted-foreground'}`}
    >
      {status === 'completed' && <Check aria-hidden="true" className="size-3.5" />}
      {status === 'transferring' && (
        <Loader2 aria-hidden="true" className="size-3.5 animate-spin" />
      )}
      {t(`clipboard.transfer.${status}`)}
      {status === 'failed' && reason && <span className="text-ui-body">{reason}</span>}
      {status === 'cancelled' && (
        <span>{t(`clipboard.transfer.cancelReason.${cancelledReason}`)}</span>
      )}
    </span>
  )
}
