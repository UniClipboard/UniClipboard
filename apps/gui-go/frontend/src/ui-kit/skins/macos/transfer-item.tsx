import type { TransferStatusItem } from '@/ui-kit/contract'
import { ProgressBar } from './entry'
import { Icon } from './icons'

/** Completed files stay compact; only active files expose byte progress. */
export function TransferItem({ item }: { item: TransferStatusItem }) {
  return (
    <li className="mac-transfer-item" data-tone={item.tone}>
      <div className="mac-transfer-file-heading">
        <span className="mac-transfer-file-icon">
          <Icon name="doc" size={16} />
        </span>
        <span className="mac-transfer-file-name" title={item.label}>
          {item.label}
        </span>
        {item.tone === 'positive' ? (
          <span className="mac-transfer-file-status">
            <Icon name="check" size={14} />
            <span className="sr-only">{item.status}</span>
          </span>
        ) : (
          <span className="mac-transfer-file-status">{item.status}</span>
        )}
      </div>
      {item.tone === 'info' && (
        <div className="mac-transfer-file-progress">
          <ProgressBar label={item.label} value={item.value} />
          <span className="mac-transfer-summary">{item.bytes}</span>
        </div>
      )}
      {item.reason && item.tone !== 'positive' && (
        <p className="mac-transfer-reason">{item.reason}</p>
      )}
    </li>
  )
}
