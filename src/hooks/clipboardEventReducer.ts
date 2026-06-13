import type { UnknownAction } from '@reduxjs/toolkit'
import {
  addPendingEntry,
  removePendingEntry,
  type PendingClipboardEntry,
} from '@/store/slices/clipboardSlice'
import {
  cancelClipboardWrite,
  linkTransferToEntry,
  markTransferCancelled,
  markTransferCompleted,
  markTransferFailed,
  normalizeCancelReason,
  setEntryTransferStatus,
  updateTransferProgress,
} from '@/store/slices/fileTransferSlice'

export interface ClipboardRealtimeEvent {
  topic: string
  eventType: string
  payload: unknown
  ts?: number
}

export type ClipboardEventReducerAction = UnknownAction

export type ClipboardEventReducerEffect =
  | { type: 'flushLocalEntries'; entryIds: string[] }
  | { type: 'scheduleLocalFlush'; delayMs: number }
  | { type: 'invalidateRemote' }
  | { type: 'scheduleRemoteInvalidate'; delayMs: number }

export interface ClipboardEventReducerResult {
  actions: ClipboardEventReducerAction[]
  effects: ClipboardEventReducerEffect[]
  state: ClipboardEventReducerState
}

export interface ClipboardEventReducerOptions {
  now: () => number
  throttleMs: number
}

export interface ClipboardEventReducerState {
  lastLocalFetchMs: number | undefined
  localFlushScheduled: boolean
  pendingLocalEntryIds: string[]
  lastRemoteInvalidateMs: number | undefined
  remoteInvalidateScheduled: boolean
}

export interface ClipboardEventReducerInput {
  nowMs: number
  throttleMs: number
}

interface ClipboardNewContentPayload {
  entryId: string
  preview: string
  origin: string
}

interface ClipboardIncomingPendingPayload {
  entryId: string
  fromDevice: string
  totalBytes?: number | null
  filenames?: string[]
}

interface FileTransferStatusEvent {
  transferId: string
  entryId: string
  status: string
  reason?: string | null
}

interface FileTransferProgressEvent {
  transferId: string
  entryId?: string | null
  peerId: string
  direction: 'Sending' | 'Receiving'
  bytesTransferred: number
  totalBytes?: number | null
}

const VALID_TRANSFER_STATUSES = [
  'pending',
  'transferring',
  'completed',
  'failed',
  'cancelled',
] as const

type ValidTransferStatus = (typeof VALID_TRANSFER_STATUSES)[number]

export function createInitialClipboardEventReducerState(): ClipboardEventReducerState {
  return {
    lastLocalFetchMs: undefined,
    localFlushScheduled: false,
    pendingLocalEntryIds: [],
    lastRemoteInvalidateMs: undefined,
    remoteInvalidateScheduled: false,
  }
}

function result(
  state: ClipboardEventReducerState,
  actions: ClipboardEventReducerAction[] = [],
  effects: ClipboardEventReducerEffect[] = []
): ClipboardEventReducerResult {
  return { actions, effects, state }
}

function isValidTransferStatus(status: string): status is ValidTransferStatus {
  return VALID_TRANSFER_STATUSES.includes(status as ValidTransferStatus)
}

export function createClipboardEventReducer({ now, throttleMs }: ClipboardEventReducerOptions) {
  let state = createInitialClipboardEventReducerState()

  function reduce(event: ClipboardRealtimeEvent): ClipboardEventReducerResult {
    const reduction = reduceClipboardRealtimeEvent(event, state, { nowMs: now(), throttleMs })
    state = reduction.state
    return reduction
  }

  function flushLocal(): ClipboardEventReducerResult {
    const reduction = flushClipboardLocal(state, { nowMs: now() })
    state = reduction.state
    return reduction
  }

  function flushRemote(): ClipboardEventReducerResult {
    const reduction = flushClipboardRemote(state, { nowMs: now() })
    state = reduction.state
    return reduction
  }

  return {
    reduce,
    flushLocal,
    flushRemote,
  }
}

export function reduceClipboardRealtimeEvent(
  event: ClipboardRealtimeEvent,
  state: ClipboardEventReducerState,
  input: ClipboardEventReducerInput
): ClipboardEventReducerResult {
  if (event.eventType === 'clipboard.incoming_pending') {
    const payload = event.payload as ClipboardIncomingPendingPayload
    const pending: PendingClipboardEntry = {
      entryId: payload.entryId,
      fromDevice: payload.fromDevice,
      totalBytes: payload.totalBytes ?? null,
      filenames: payload.filenames ?? [],
      createdAt: input.nowMs,
    }
    return result(state, [
      addPendingEntry(pending),
      setEntryTransferStatus({
        entryId: payload.entryId,
        status: 'transferring',
        reason: null,
      }),
    ])
  }

  if (event.eventType === 'clipboard.new_content') {
    const payload = event.payload as ClipboardNewContentPayload
    const actions = [removePendingEntry(payload.entryId), cancelClipboardWrite()]
    if (payload.origin === 'local') {
      const local = reduceLocalNewContent(payload.entryId, state, input)
      return result(local.state, actions, local.effects)
    }
    const remote = reduceRemoteNewContent(state, input)
    return result(remote.state, actions, remote.effects)
  }

  if (event.eventType === 'file-transfer.status_changed') {
    return result(state, reduceTransferStatus(event.payload as FileTransferStatusEvent))
  }

  if (event.eventType === 'file-transfer.progress') {
    return result(
      state,
      reduceTransferProgress(event.payload as FileTransferProgressEvent, event.ts)
    )
  }

  return result(state)
}

export function flushClipboardLocal(
  state: ClipboardEventReducerState,
  input: Pick<ClipboardEventReducerInput, 'nowMs'>
): ClipboardEventReducerResult {
  const nextState = {
    ...state,
    localFlushScheduled: false,
    lastLocalFetchMs: state.pendingLocalEntryIds.length > 0 ? input.nowMs : state.lastLocalFetchMs,
    pendingLocalEntryIds: [],
  }
  const effects =
    state.pendingLocalEntryIds.length > 0
      ? [{ type: 'flushLocalEntries' as const, entryIds: state.pendingLocalEntryIds }]
      : []
  return result(nextState, [], effects)
}

export function flushClipboardRemote(
  state: ClipboardEventReducerState,
  input: Pick<ClipboardEventReducerInput, 'nowMs'>
): ClipboardEventReducerResult {
  return result(
    {
      ...state,
      remoteInvalidateScheduled: false,
      lastRemoteInvalidateMs: input.nowMs,
    },
    [],
    [{ type: 'invalidateRemote' }]
  )
}

function reduceLocalNewContent(
  entryId: string,
  state: ClipboardEventReducerState,
  input: ClipboardEventReducerInput
): Pick<ClipboardEventReducerResult, 'state' | 'effects'> {
  const pendingLocalEntryIds = state.pendingLocalEntryIds.includes(entryId)
    ? state.pendingLocalEntryIds
    : [...state.pendingLocalEntryIds, entryId]
  const sinceLast =
    state.lastLocalFetchMs === undefined
      ? Number.POSITIVE_INFINITY
      : input.nowMs - state.lastLocalFetchMs

  if (sinceLast >= input.throttleMs) {
    const flushResult = flushClipboardLocal(
      { ...state, localFlushScheduled: false, pendingLocalEntryIds },
      { nowMs: input.nowMs }
    )
    return { state: flushResult.state, effects: flushResult.effects }
  }

  if (!state.localFlushScheduled) {
    return {
      state: { ...state, localFlushScheduled: true, pendingLocalEntryIds },
      effects: [{ type: 'scheduleLocalFlush', delayMs: input.throttleMs - sinceLast }],
    }
  }

  return {
    state: { ...state, pendingLocalEntryIds },
    effects: [],
  }
}

function reduceRemoteNewContent(
  state: ClipboardEventReducerState,
  input: ClipboardEventReducerInput
): Pick<ClipboardEventReducerResult, 'state' | 'effects'> {
  const sinceLast =
    state.lastRemoteInvalidateMs === undefined
      ? Number.POSITIVE_INFINITY
      : input.nowMs - state.lastRemoteInvalidateMs

  if (sinceLast >= input.throttleMs) {
    return {
      state: {
        ...state,
        remoteInvalidateScheduled: false,
        lastRemoteInvalidateMs: input.nowMs,
      },
      effects: [{ type: 'invalidateRemote' }],
    }
  }

  if (!state.remoteInvalidateScheduled) {
    return {
      state: { ...state, remoteInvalidateScheduled: true },
      effects: [{ type: 'scheduleRemoteInvalidate', delayMs: input.throttleMs - sinceLast }],
    }
  }

  return { state, effects: [] }
}

function reduceTransferStatus(payload: FileTransferStatusEvent): ClipboardEventReducerAction[] {
  const actions: ClipboardEventReducerAction[] = [
    linkTransferToEntry({ transferId: payload.transferId, entryId: payload.entryId }),
  ]

  if (isValidTransferStatus(payload.status)) {
    actions.push(
      setEntryTransferStatus({
        entryId: payload.entryId,
        status: payload.status,
        reason: payload.reason ?? null,
      })
    )
  }

  if (payload.status === 'failed') {
    actions.push(
      markTransferFailed({
        transferId: payload.transferId,
        error: payload.reason ?? undefined,
      })
    )
  } else if (payload.status === 'cancelled') {
    actions.push(
      removePendingEntry(payload.entryId),
      markTransferCancelled({
        transferId: payload.transferId,
        reason: normalizeCancelReason(payload.reason),
      })
    )
  } else if (payload.status === 'completed') {
    actions.push(markTransferCompleted({ transferId: payload.transferId }))
  }

  return actions
}

function reduceTransferProgress(
  payload: FileTransferProgressEvent,
  eventTs: number | undefined
): ClipboardEventReducerAction[] {
  const actions: ClipboardEventReducerAction[] = [
    updateTransferProgress({
      transferId: payload.transferId,
      entryId: payload.entryId ?? null,
      peerId: payload.peerId,
      direction: payload.direction,
      bytesTransferred: payload.bytesTransferred,
      totalBytes: payload.totalBytes ?? null,
      eventTs,
    }),
  ]

  if (payload.entryId) {
    actions.push(linkTransferToEntry({ transferId: payload.transferId, entryId: payload.entryId }))
  }

  return actions
}
