import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { __test__ } from '@/components/device/SpaceMembersPanel'
import i18n from '@/i18n'

vi.mock('@/hooks/useSetting', () => ({
  useSetting: () => ({
    setting: {
      network: { allowRelayFallback: true },
      sync: { autoSync: true },
      fileSync: { fileSyncEnabled: true },
    },
  }),
}))

vi.mock('@/lib/daemon-ws', () => ({
  daemonWs: {
    subscribe: vi.fn(() => vi.fn()),
  },
}))

vi.mock('@/store/hooks', () => ({
  useAppDispatch: () => vi.fn(),
  useAppSelector: () => ({
    spaceMembers: [],
    spaceMembersError: null,
    localDevice: null,
  }),
}))

const { DeviceRow } = __test__

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  cleanup()
})

describe('DeviceRow connection address', () => {
  it('显示直连 IP 地址', () => {
    render(
      <DeviceRow
        deviceName="Fedora"
        connected={true}
        channel="direct"
        connectionAddress="100.117.177.15:44868"
        lanOnlyActive={false}
        onClick={() => undefined}
      />
    )

    expect(screen.getByText('Fedora')).toBeInTheDocument()
    expect(screen.getByText('直连')).toBeInTheDocument()
    expect(screen.getByText('100.117.177.15:44868')).toBeInTheDocument()
  })

  it('显示中转地址', () => {
    render(
      <DeviceRow
        deviceName="Mac"
        connected={true}
        channel="relay"
        connectionAddress="https://usw1-1.relay.n0.iroh-canary.iroh.link./"
        lanOnlyActive={false}
        onClick={() => undefined}
      />
    )

    expect(screen.getByText('中转')).toBeInTheDocument()
    expect(screen.getByText(/usw1-1\.relay\.n0\.iroh-canary\.iroh\.link/)).toBeInTheDocument()
  })
})
