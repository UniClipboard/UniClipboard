import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import type { EntryDeliveryView } from '@/api/tauri-command/clipboard_delivery'
import EntryDeliverySection from '@/components/clipboard/EntryDeliverySection'
import i18n from '@/i18n'

/** "正常本地 entry,N 个对端混合状态" 是 detail 区域最常见也最容易回归的形态,
 * 用它做基线确保 source/list/status/reason 四块文案都能拼对。 */
function mixedDelivery(): EntryDeliveryView {
  return {
    entryId: 'entry-001',
    source: { tag: 'local' },
    deliveries: [
      {
        targetDeviceId: 'did_a1b2c3d4e5',
        targetDeviceName: null,
        status: { tag: 'delivered' },
        reasonDetail: null,
        updatedAtMs: 1_700_000_000_000,
      },
      {
        targetDeviceId: 'did_f6g7h8i9j0',
        targetDeviceName: null,
        status: { tag: 'duplicate' },
        reasonDetail: null,
        updatedAtMs: 1_700_000_000_001,
      },
      {
        targetDeviceId: 'did_k1l2m3n4o5',
        targetDeviceName: null,
        status: { tag: 'failed', reason: 'offline' },
        reasonDetail: 'no route to host',
        updatedAtMs: 1_700_000_000_002,
      },
      {
        targetDeviceId: 'did_p6q7r8s9t0',
        targetDeviceName: null,
        status: { tag: 'pending' },
        reasonDetail: null,
        updatedAtMs: null,
      },
    ],
  }
}

describe('EntryDeliverySection', () => {
  it('returns null when delivery view is unavailable', () => {
    const { container } = render(<EntryDeliverySection delivery={null} />)
    expect(container).toBeEmptyDOMElement()
  })

  it('renders local source + four-status row mix', () => {
    render(<EntryDeliverySection delivery={mixedDelivery()} />)

    // Source 行: "来自: 本地"
    expect(screen.getByText(i18n.t('delivery.source.label'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('delivery.source.local'))).toBeInTheDocument()

    // 四行设备截断展示 (前 8 字符 + …)
    expect(screen.getByText('did_a1b2…')).toBeInTheDocument()
    expect(screen.getByText('did_f6g7…')).toBeInTheDocument()
    expect(screen.getByText('did_k1l2…')).toBeInTheDocument()
    expect(screen.getByText('did_p6q7…')).toBeInTheDocument()

    // 四档状态文案,失败要带 reason
    expect(screen.getByText(i18n.t('delivery.status.delivered'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('delivery.status.duplicate'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('delivery.status.pending'))).toBeInTheDocument()
    expect(
      screen.getByText(
        i18n.t('delivery.status.failedWithReason', {
          reason: i18n.t('delivery.failureReason.offline'),
        })
      )
    ).toBeInTheDocument()

    const list = screen.getByRole('list')
    expect(within(list).getAllByRole('listitem')).toHaveLength(4)
  })

  it('shows historical hint and hides device list for legacy entries', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-old',
      source: { tag: 'historical' },
      deliveries: [],
    }
    render(<EntryDeliverySection delivery={delivery} />)

    expect(screen.getByText(i18n.t('delivery.source.historical'))).toBeInTheDocument()
    expect(screen.getByText(i18n.t('delivery.list.historical'))).toBeInTheDocument()
    expect(screen.queryByRole('list')).not.toBeInTheDocument()
  })

  it('shows "no peers" hint for local entry without trusted peers', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-solo',
      source: { tag: 'local' },
      deliveries: [],
    }
    render(<EntryDeliverySection delivery={delivery} />)

    expect(screen.getByText(i18n.t('delivery.list.noPeers'))).toBeInTheDocument()
  })

  it('renders remote source with truncated device id when name is missing', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-remote',
      source: { tag: 'remote', deviceId: 'did_sender_xyz', deviceName: null },
      deliveries: [
        {
          targetDeviceId: 'did_peer_aaa',
          targetDeviceName: null,
          status: { tag: 'delivered' },
          reasonDetail: null,
          updatedAtMs: 1_700_000_000_000,
        },
      ],
    }
    render(<EntryDeliverySection delivery={delivery} />)

    // 名字缺失时 fallback 到 device_id 截断
    expect(screen.getByText('did_send…')).toBeInTheDocument()
  })

  it('prefers device names over device ids when resolved', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-named',
      source: { tag: 'remote', deviceId: 'did_sender_xyz', deviceName: 'Mac Studio' },
      deliveries: [
        {
          targetDeviceId: 'did_target_aaa',
          targetDeviceName: 'iPad Pro',
          status: { tag: 'delivered' },
          reasonDetail: null,
          updatedAtMs: 1_700_000_000_000,
        },
        {
          // 空白名应被视为缺失,fallback 到 id 截断
          targetDeviceId: 'did_target_bbb',
          targetDeviceName: '   ',
          status: { tag: 'pending' },
          reasonDetail: null,
          updatedAtMs: null,
        },
      ],
    }
    render(<EntryDeliverySection delivery={delivery} />)

    // 来源用真实 name 而不是截断 id
    expect(screen.getByText('Mac Studio')).toBeInTheDocument()
    expect(screen.queryByText('did_send…')).not.toBeInTheDocument()

    // 第一台 target 用真实名;第二台 fallback 到截断 id
    expect(screen.getByText('iPad Pro')).toBeInTheDocument()
    expect(screen.getByText('did_targ…')).toBeInTheDocument()
  })

  it('maps all five failure reasons to their i18n labels', () => {
    const reasons = ['offline', 'localPolicy', 'peerRejected', 'io', 'internal'] as const
    const delivery: EntryDeliveryView = {
      entryId: 'entry-failures',
      source: { tag: 'local' },
      deliveries: reasons.map((reason, idx) => ({
        targetDeviceId: `did_peer_${idx}xxxxxxxx`,
        targetDeviceName: null,
        status: { tag: 'failed', reason },
        reasonDetail: null,
        updatedAtMs: 1_700_000_000_000 + idx,
      })),
    }
    render(<EntryDeliverySection delivery={delivery} />)

    for (const reason of reasons) {
      const label = i18n.t('delivery.status.failedWithReason', {
        reason: i18n.t(`delivery.failureReason.${reason}`),
      })
      expect(screen.getByText(label)).toBeInTheDocument()
    }
  })
})
