import { useTranslation } from 'react-i18next'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'
import { ProgressBar } from '@/ui-kit'
import { formatFileSize } from '@/utils'

export default function FilePreviewProgress({ transfer }: { transfer: TransferProgressInfo }) {
  const { t } = useTranslation()
  return (
    <div className="flex flex-col gap-2 border-b border-border/50 px-4 py-3">
      <span className="text-ui-caption text-muted-foreground">
        {t(
          transfer.direction === 'sending'
            ? 'clipboard.transfer.sending'
            : 'clipboard.transfer.receiving'
        )}
        {' · '}
        {formatFileSize(transfer.bytesTransferred)}
        {transfer.totalBytes != null &&
          transfer.totalBytes > 0 &&
          ` / ${formatFileSize(transfer.totalBytes)}`}
      </span>
      <ProgressBar
        label={t('clipboard.transfer.transferring')}
        value={
          transfer.totalBytes && transfer.totalBytes > 0
            ? Math.min(100, Math.max(0, (transfer.bytesTransferred / transfer.totalBytes) * 100))
            : undefined
        }
      />
    </div>
  )
}
