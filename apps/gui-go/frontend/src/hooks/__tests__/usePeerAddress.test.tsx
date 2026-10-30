import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { validatePeerAddress } from '@/components/device/peer-address-utils'
import { usePeerAddress } from '@/hooks/usePeerAddress'
import {
  awaitNetworkSettingsSaves,
  resetNetworkRestartStateForTest,
} from '@/store/networkRestartStore'

const mocks = vi.hoisted(() => ({
  update: vi.fn(),
  saved: { ip: '10.10.0.2', port: 51820 } as { ip: string; port: number } | undefined,
}))
vi.mock('@/hooks/useSetting', () => ({
  useSettingSelector: (selector: (context: unknown) => unknown) =>
    selector({
      setting: {
        network: { peerAddresses: { peer: mocks.saved, other: { ip: '10.10.0.3', port: 1234 } } },
      },
      loading: false,
      error: null,
      updateNetworkSetting: mocks.update,
    }),
}))
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))

describe('peer address validation', () => {
  it.each([
    '10.10.0.2',
    '192.168.1.2',
    '::1',
    'fd00::2',
    'ff::',
    '::ffff:192.168.1.2',
    '2001:db8:1:2:3:4:5:6',
  ])('accepts unicast literal %s', ip => {
    expect(validatePeerAddress(ip, '51820')).toEqual({ valid: true, address: { ip, port: 51820 } })
  })
  it('trims whitespace and strips IPv6 brackets', () => {
    expect(validatePeerAddress(' [fd00::2] ', ' 65535 ')).toEqual({
      valid: true,
      address: { ip: 'fd00::2', port: 65535 },
    })
  })
  it.each([
    'host.example',
    '1.2.3',
    '1234',
    '01.2.3.4',
    '256.1.1.1',
    '0.0.0.0',
    '239.0.0.1',
    '::',
    '::0.0.0.0',
    'ff02::1',
    '[10.1.1.2]',
    '[::1',
    'fd00::x',
    '1::2::3',
    'fe80::1%en0',
    '::1:51820',
    '0x7f000001',
    '1.2.3.4/path',
    '1:2:3:4:5:6:7:8:9',
  ])('rejects invalid or non-unicast IP %s', ip => {
    expect(validatePeerAddress(ip, '1234')).toEqual({ valid: false, field: 'ip' })
  })
  it.each(['', '0', '65536', '-1', '1.5', '1e3', 'port'])('rejects invalid port %s', port => {
    expect(validatePeerAddress('10.10.0.2', port)).toEqual({ valid: false, field: 'port' })
  })
})

describe('usePeerAddress', () => {
  beforeEach(() => {
    resetNetworkRestartStateForTest()
    mocks.saved = { ip: '10.10.0.2', port: 51820 }
    mocks.update.mockReset().mockImplementation(async patch => {
      mocks.saved = patch.peerAddresses.peer ?? undefined
      return { restartRequired: false }
    })
  })
  it('saves only this device, then shows the authoritative value', async () => {
    const { result } = renderHook(() => usePeerAddress('peer'))
    act(() => {
      result.current.setIp(' [fd00::2] ')
    })
    await act(async () => {
      await result.current.save()
    })
    expect(mocks.update).toHaveBeenCalledWith({
      peerAddresses: { peer: { ip: 'fd00::2', port: 51820 } },
    })
    expect(result.current.ip).toBe('fd00::2')
    expect(result.current.canSave).toBe(false)
  })
  it('clears with a null entry and leaves other devices alone', async () => {
    const { result, rerender } = renderHook(() => usePeerAddress('peer'))
    await act(async () => {
      await result.current.clear()
    })
    rerender()
    expect(mocks.update).toHaveBeenCalledWith({ peerAddresses: { peer: null } })
    expect(result.current.ip).toBe('')
    expect(result.current.port).toBe('')
  })
  it('never sends invalid input', async () => {
    const { result } = renderHook(() => usePeerAddress('peer'))
    act(() => {
      result.current.setIp('host.example')
    })
    await act(async () => {
      await result.current.save()
    })
    expect(result.current.ipError).toBe('devices.directAddress.invalidIp')
    expect(mocks.update).not.toHaveBeenCalled()
  })
  it('shows rejection and releases in-flight tracking so retry can succeed', async () => {
    mocks.update.mockRejectedValueOnce(
      new Error('invalid peer address: the device id is not valid')
    )
    const { result } = renderHook(() => usePeerAddress('peer'))
    act(() => {
      result.current.setPort('6000')
    })
    await act(async () => {
      await result.current.save()
    })
    expect(result.current.error).toBe('invalid peer address: the device id is not valid')
    expect(result.current.busy).toBe(false)
    await expect(awaitNetworkSettingsSaves()).resolves.toBeUndefined()
    await act(async () => {
      await result.current.save()
    })
    expect(mocks.update).toHaveBeenCalledTimes(2)
    expect(result.current.error).toBeNull()
  })
  it('tracks and deduplicates writes until completion', async () => {
    let resolve!: (result: { restartRequired: boolean }) => void
    mocks.update.mockReturnValue(
      new Promise(done => {
        resolve = done
      })
    )
    const { result } = renderHook(() => usePeerAddress('peer'))
    act(() => {
      result.current.setPort('6000')
    })
    let save!: Promise<void>
    act(() => {
      save = result.current.save()
      void result.current.save()
    })
    expect(result.current.busy).toBe(true)
    expect(mocks.update).toHaveBeenCalledTimes(1)
    let drained = false
    const drain = awaitNetworkSettingsSaves().then(() => {
      drained = true
    })
    await Promise.resolve()
    expect(drained).toBe(false)
    await act(async () => {
      resolve({ restartRequired: false })
      await save
      await drain
    })
    expect(result.current.busy).toBe(false)
  })
})
