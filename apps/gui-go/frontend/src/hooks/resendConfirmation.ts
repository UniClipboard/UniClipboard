/**
 * Pending "start a new transfer?" confirmations for resend.
 *
 * A resend first reconciles with the existing task. Only after the user
 * explicitly confirms may the host ask the daemon to start a new transfer
 * (`confirmNewTransfer`). Requests from several entries (bulk resend) are
 * merged by the caller into one request, so the user answers once.
 *
 * Module-level store (same pattern as the in-flight store in
 * `useResendAction`): the dialog host and the hook share one source of truth.
 */

import { useSyncExternalStore } from 'react'

export interface ResendConfirmationRequest {
  /** Number of entries that would be sent again. */
  entryCount: number
  /** Distinct target devices that would receive a new transfer. */
  targetDeviceIds: string[]
  /** At least one target may end up with duplicate files. */
  duplicateFilesPossible: boolean
}

interface QueuedRequest {
  request: ResendConfirmationRequest
  resolve: (confirmed: boolean) => void
}

const queue: QueuedRequest[] = []
const listeners = new Set<() => void>()
let head: ResendConfirmationRequest | null = null

function publish(): void {
  head = queue[0]?.request ?? null
  for (const listener of Array.from(listeners)) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function getSnapshot(): ResendConfirmationRequest | null {
  return head
}

/** Ask the user; resolves `true` only on an explicit confirmation. */
export function requestResendConfirmation(request: ResendConfirmationRequest): Promise<boolean> {
  return new Promise<boolean>(resolve => {
    queue.push({ request, resolve })
    publish()
  })
}

/** Answer the request currently shown by the dialog. */
export function answerResendConfirmation(confirmed: boolean): void {
  const answered = queue.shift()
  if (!answered) return
  publish()
  answered.resolve(confirmed)
}

/** The request the dialog should show, or `null` when none is pending. */
export function useResendConfirmationRequest(): ResendConfirmationRequest | null {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
}

/** @internal Test hook: decline everything pending and clear the queue. */
export function __resetResendConfirmationForTests(): void {
  while (queue.length > 0) answerResendConfirmation(false)
}
