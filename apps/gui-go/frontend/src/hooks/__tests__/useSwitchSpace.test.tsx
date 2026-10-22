import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SwitchSpaceResponse } from '@/api/daemon/setupV2'
import { useSwitchSpace } from '@/hooks/useSwitchSpace'

const mocks = vi.hoisted(() => ({
  switchSpace: vi.fn(),
  cancelJoinSpace: vi.fn(),
  dispatch: vi.fn(),
  admission: vi.fn(),
}))
vi.mock('@/api/daemon/setupV2', () => ({
  switchSpace: mocks.switchSpace,
  cancelJoinSpace: mocks.cancelJoinSpace,
  SetupV2Error: class extends Error {},
}))
vi.mock('@/hooks/useJoinAdmission', () => ({ useJoinAdmission: mocks.admission }))
vi.mock('@/store/hooks', () => ({ useAppDispatch: () => mocks.dispatch }))
vi.mock('@/store/slices/devicesSlice', () => ({
  fetchSpaceMembers: () => ({ type: 'members' }),
  fetchLocalDeviceInfo: () => ({ type: 'local' }),
}))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

const pending = (joinId: string) => ({
  status: 'pending' as const,
  joinId,
  targetSpaceId: null,
  sponsorDeviceId: null,
  sponsorIdentityFingerprint: null,
  cancelRequested: false,
})
const active = (joinId: string) => ({
  status: 'active' as const,
  joinId,
  joinedSpace: {
    migratedRecords: 1,
    preservedUnreadableRecords: 0,
    sponsorDeviceId: 'sponsor',
    sponsorIdentityFingerprint: 'sponsor-fingerprint',
    spaceId: 'space',
    selfDeviceId: 'self',
    selfIdentityFingerprint: 'self-fingerprint',
  },
})

describe('useSwitchSpace operation ownership', () => {
  beforeEach(() => vi.clearAllMocks())

  function setup() {
    return renderHook(() => useSwitchSpace({ onOpenChange: vi.fn(), autoClose: false }))
  }

  async function submit(result: ReturnType<typeof setup>['result'], joinId: string) {
    mocks.switchSpace.mockResolvedValueOnce(pending(joinId))
    act(() => {
      result.current.setCode('012345')
      result.current.setPass('passphrase')
    })
    await act(() => result.current.handleSubmit())
  }

  it('ignores old admission and progress results after reset and a new join', async () => {
    const { result } = setup()
    await submit(result, 'old')
    const [, resolved, progress] = mocks.admission.mock.lastCall!
    act(() => result.current.reset())
    await submit(result, 'new')
    act(() => {
      progress({ ...pending('old'), status: 'processing' })
      resolved(active('old'))
    })
    expect(result.current.step).toBe('pending')
    expect(result.current.pendingJoinId).toBe('new')
    expect(mocks.dispatch).not.toHaveBeenCalled()
    act(() => mocks.admission.mock.lastCall![1](active('new')))
    expect(result.current.step).toBe('success')
    expect(mocks.dispatch).toHaveBeenCalledTimes(2)
  })

  it.each(['result', 'error'] as const)('ignores a stale cancellation %s', async outcome => {
    const { result } = setup()
    await submit(result, 'old')
    const cancellation = deferred<SwitchSpaceResponse>()
    mocks.cancelJoinSpace.mockReturnValueOnce(cancellation.promise)
    let request!: Promise<void>
    act(() => {
      request = result.current.handleCancelPending()
    })
    act(() => result.current.reset())
    await submit(result, 'new')
    await act(async () => {
      if (outcome === 'result') cancellation.resolve(active('old'))
      else cancellation.reject(new Error('old cancellation failed'))
      await request
    })
    expect(result.current.step).toBe('pending')
    expect(result.current.pendingJoinId).toBe('new')
    expect(mocks.dispatch).not.toHaveBeenCalled()
  })

  it('stops observing a join after cancellation fails', async () => {
    const { result } = setup()
    await submit(result, 'join')
    const [, resolved, progress] = mocks.admission.mock.lastCall!
    mocks.cancelJoinSpace.mockRejectedValueOnce(new Error('cancellation failed'))
    await act(() => result.current.handleCancelPending())
    act(() => {
      progress(pending('join'))
      resolved(active('join'))
    })
    expect(result.current.step).toBe('failed')
    expect(result.current.pendingJoinId).toBeNull()
    expect(mocks.dispatch).not.toHaveBeenCalled()
  })

  it.each(['result', 'error'] as const)(
    'ignores a submit %s arriving after reset',
    async outcome => {
      const { result } = setup()
      const response = deferred<SwitchSpaceResponse>()
      mocks.switchSpace.mockReturnValueOnce(response.promise)
      act(() => {
        result.current.setCode('012345')
        result.current.setPass('passphrase')
      })
      let request!: Promise<void>
      act(() => {
        request = result.current.handleSubmit()
      })
      act(() => result.current.reset())
      await act(async () => {
        if (outcome === 'result') response.resolve(pending('old'))
        else response.reject(new Error('old submission failed'))
        await request
      })
      expect(result.current.step).toBe('input')
      expect(result.current.pendingJoinId).toBeNull()
      expect(mocks.dispatch).not.toHaveBeenCalled()
    }
  )
})
