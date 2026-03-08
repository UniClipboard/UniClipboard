import { createSlice, PayloadAction } from '@reduxjs/toolkit'
import { RootState } from '@/store'

export interface TransferProgressPayload {
  transferId: string
  peerId: string
  direction: 'Sending' | 'Receiving'
  chunksCompleted: number
  totalChunks: number
  bytesTransferred: number
  totalBytes: number
}

interface TransferState extends TransferProgressPayload {
  updatedAt: number
}

interface TransferSliceState {
  activeTransfers: Record<string, TransferState>
}

const initialState: TransferSliceState = {
  activeTransfers: {},
}

const transferSlice = createSlice({
  name: 'transfer',
  initialState,
  reducers: {
    updateTransferProgress(state, action: PayloadAction<TransferProgressPayload>) {
      const payload = action.payload
      state.activeTransfers[payload.transferId] = {
        ...payload,
        updatedAt: Date.now(),
      }
    },
    clearTransfer(state, action: PayloadAction<string>) {
      delete state.activeTransfers[action.payload]
    },
    clearStaleTransfers(state) {
      const now = Date.now()
      const staleThreshold = 5000 // 5 seconds
      for (const [id, transfer] of Object.entries(state.activeTransfers)) {
        if (
          transfer.chunksCompleted === transfer.totalChunks &&
          now - transfer.updatedAt > staleThreshold
        ) {
          delete state.activeTransfers[id]
        }
      }
    },
  },
})

export const { updateTransferProgress, clearTransfer, clearStaleTransfers } = transferSlice.actions

export const selectActiveTransfers = (state: RootState): TransferState[] =>
  Object.values(state.transfer.activeTransfers)
    .filter(t => t.chunksCompleted < t.totalChunks)
    .sort((a, b) => b.updatedAt - a.updatedAt)

export const selectAllTransfers = (state: RootState): TransferState[] =>
  Object.values(state.transfer.activeTransfers).sort((a, b) => b.updatedAt - a.updatedAt)

export default transferSlice.reducer
