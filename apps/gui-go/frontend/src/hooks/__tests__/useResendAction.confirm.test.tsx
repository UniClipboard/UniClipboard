/**
 * Resend reconciles with the existing task first. A new transfer is only
 * started after one explicit confirmation, and only for targets the daemon
 * allows to be confirmed (settled / not retained / unsupported).
 */

import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ResendEntryReportDto,
  ResendReconciliationState,
} from '@/api/tauri-command/clipboard_delivery'
import {
  __resetResendConfirmationForTests,
  answerResendConfirmation,
} from '@/hooks/resendConfirmation'
import { __resetResendActionStoreForTests, useResendAction } from '@/hooks/useResendAction'

const resendEntryMock = vi.fn()
const toastSuccessMock = vi.fn()
const toastErrorMock = vi.fn()
const toastInfoMock = vi.fn()

vi.mock('@/api/tauri-command/clipboard_delivery', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/clipboard_delivery')>(
    '@/api/tauri-command/clipboard_delivery'
  )
  return { ...actual, resendEntry: (...args: unknown[]) => resendEntryMock(...args) }
})

vi.mock('@/components/ui/toast', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}))

function report(
  states: Array<[string, ResendReconciliationState]>,
  counts: Partial<ResendEntryReportDto> = {}
): ResendEntryReportDto {
  return {
    accepted: 0,
    duplicate: 0,
    offline: 0,
    errored: 0,
    pending: 0,
    ...counts,
    reconciled: states.map(([targetDeviceId, state]) => ({
      targetDeviceId,
      state,
      newTransferRequested: false,
      duplicateFilesPossible: state !== 'running',
    })),
  }
}

beforeEach(() => {
  resendEntryMock.mockReset()
  toastSuccessMock.mockReset()
  toastErrorMock.mockReset()
  toastInfoMock.mockReset()
  __resetResendActionStoreForTests()
  __resetResendConfirmationForTests()
})

describe('useResendAction — reconcile then confirm', () => {
  it('reconciles only (no confirmation) for a running task and never asks', async () => {
    resendEntryMock.mockResolvedValueOnce(report([['dev-a', 'running']]))
    const { result } = renderHook(() => useResendAction())

    await act(async () => {
      await result.current.resendAll('entry-1')
    })

    expect(resendEntryMock).toHaveBeenCalledTimes(1)
    expect(resendEntryMock.mock.calls[0][0]).not.toHaveProperty('confirmNewTransfer', true)
    expect(toastInfoMock).toHaveBeenCalledTimes(1)
    expect(toastSuccessMock).not.toHaveBeenCalled()
  })

  it('does not start a new transfer when the user declines', async () => {
    resendEntryMock.mockResolvedValueOnce(report([['dev-a', 'not_retained']]))
    const { result } = renderHook(() => useResendAction())

    let done!: Promise<void>
    act(() => {
      done = result.current.resendAll('entry-1')
    })
    await waitFor(() => expect(resendEntryMock).toHaveBeenCalledTimes(1))
    await act(async () => {
      answerResendConfirmation(false)
      await done
    })

    expect(resendEntryMock).toHaveBeenCalledTimes(1)
    expect(toastSuccessMock).not.toHaveBeenCalled()
    expect(toastInfoMock).toHaveBeenCalledTimes(1)
  })

  it('confirms once and resends only the confirmable targets', async () => {
    resendEntryMock
      .mockResolvedValueOnce(
        report([
          ['dev-a', 'settled'],
          ['dev-b', 'running'],
          ['dev-c', 'unreachable'],
        ])
      )
      .mockResolvedValueOnce({
        ...report([['dev-a', 'settled']], { accepted: 1 }),
        reconciled: [
          {
            targetDeviceId: 'dev-a',
            state: 'settled',
            newTransferRequested: true,
            duplicateFilesPossible: true,
          },
        ],
      })
    const { result } = renderHook(() => useResendAction())

    let done!: Promise<void>
    act(() => {
      done = result.current.resendAll('entry-1')
    })
    await waitFor(() => expect(resendEntryMock).toHaveBeenCalledTimes(1))
    await act(async () => {
      answerResendConfirmation(true)
      await done
    })

    expect(resendEntryMock).toHaveBeenCalledTimes(2)
    expect(resendEntryMock.mock.calls[1][0]).toEqual({
      entryId: 'entry-1',
      targetDeviceIds: ['dev-a'],
      confirmNewTransfer: true,
    })
    expect(toastSuccessMock).toHaveBeenCalledTimes(1)
  })

  it('never offers confirmation for busy, unreachable or superseded targets', async () => {
    resendEntryMock.mockResolvedValueOnce(
      report([
        ['dev-a', 'busy'],
        ['dev-b', 'unreachable'],
        ['dev-c', 'superseded'],
      ])
    )
    const { result } = renderHook(() => useResendAction())

    await act(async () => {
      await result.current.resendAll('entry-1')
    })

    expect(resendEntryMock).toHaveBeenCalledTimes(1)
    expect(toastInfoMock).toHaveBeenCalledTimes(1)
  })
})
