import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import TransferProgressBar from '@/components/clipboard/TransferProgressBar'
import type { TransferProgressInfo } from '@/store/slices/fileTransferSlice'

const progress: TransferProgressInfo = {
  transferId: 'transfer-1',
  entryId: 'entry-1',
  attemptId: 'attempt-1',
  peerId: 'peer-1',
  direction: 'Receiving',
  bytesTransferred: 38,
  totalBytes: 100,
  status: 'active',
  startedAt: 1,
  updatedAt: 2,
  bytesPerSecond: 6_291_456,
  estimatedRemainingSeconds: 12,
}

describe('TransferProgressBar compact layout', () => {
  it('keeps percent and cancel visible without a speed label', () => {
    render(
      <div className="w-[280px]">
        <TransferProgressBar progress={progress} variant="compact" onCancel={vi.fn()} />
      </div>
    )

    expect(screen.getByText('38%')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Cancel transfer' })).toBeInTheDocument()
    expect(screen.queryByText('6.00 MB/s')).not.toBeInTheDocument()
  })
})
