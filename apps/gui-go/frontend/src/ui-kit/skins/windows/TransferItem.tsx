import { CheckmarkRegular, DocumentRegular } from '@fluentui/react-icons'
import type { TransferStatusItem } from '@/ui-kit/contract'
import { ProgressBar } from './ProgressBar'
import { useTransferStyles } from './transfer-styles'

/** WinUI file row, with progress only while the file is active. */
export function TransferItem({ item }: { item: TransferStatusItem }) {
  const styles = useTransferStyles()
  return (
    <li className={styles.item} data-tone={item.tone}>
      <div className={styles.fileHeading}>
        <span className={styles.fileIcon}>
          <DocumentRegular aria-hidden="true" />
        </span>
        <span className={styles.fileName} data-tone={item.tone} title={item.label}>
          {item.label}
        </span>
        <span className={styles.fileStatus} data-tone={item.tone}>
          {item.tone === 'positive' ? (
            <>
              <CheckmarkRegular aria-hidden="true" />
              <span className="sr-only">{item.status}</span>
            </>
          ) : (
            item.status
          )}
        </span>
      </div>
      {item.tone === 'info' && (
        <div className={styles.progress}>
          <ProgressBar label={item.label} value={item.value} />
          <span className={styles.summary}>{item.bytes}</span>
        </div>
      )}
      {item.reason && item.tone !== 'positive' && <p className={styles.reason}>{item.reason}</p>}
    </li>
  )
}
