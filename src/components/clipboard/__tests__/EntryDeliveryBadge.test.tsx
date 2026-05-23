import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  DeliveryFailureReason,
  EntryDeliveryTargetView,
  EntryDeliveryView,
  ResendEntryReportDto,
} from '@/api/tauri-command/clipboard_delivery'
import EntryDeliveryBadge from '@/components/clipboard/EntryDeliveryBadge'
import { __resetResendActionStoreForTests } from '@/hooks/useResendAction'
import i18n from '@/i18n'

// commit F: 后端 resendEntry / sonner toast 都是副作用,测试中以可观察 spy
// 替换。`isResendEntryError` 是纯函数 type guard,保留真实实现,Rust typed
// error envelope 的 shape 测试由这里的 thrown-error 案例覆盖。
const resendEntryMock = vi.fn()
const toastSuccessMock = vi.fn()
const toastErrorMock = vi.fn()

vi.mock('@/api/tauri-command/clipboard_delivery', async () => {
  const actual = await vi.importActual<typeof import('@/api/tauri-command/clipboard_delivery')>(
    '@/api/tauri-command/clipboard_delivery'
  )
  return {
    ...actual,
    resendEntry: (...args: unknown[]) => resendEntryMock(...args),
  }
})

vi.mock('sonner', () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}))

// Phase 4: quick-panel 切到 EntryDeliveryBadge 后,渲染契约保护从原来的
// EntryDeliverySection 迁移到这里。覆盖三块:source 三档、summary 五档、
// popover 明细 + failure reason。

function target(
  id: string,
  name: string | null,
  status: EntryDeliveryTargetView['status'],
  reasonDetail: string | null = null
): EntryDeliveryTargetView {
  return {
    targetDeviceId: id,
    targetDeviceName: name,
    status,
    reasonDetail,
    updatedAtMs: 1_700_000_000_000,
  }
}

function mixedDelivery(): EntryDeliveryView {
  return {
    entryId: 'entry-001',
    source: { tag: 'local' },
    deliveries: [
      target('did_a1b2c3d4e5', null, { tag: 'delivered' }),
      target('did_f6g7h8i9j0', null, { tag: 'duplicate' }),
      target('did_k1l2m3n4o5', null, { tag: 'failed', reason: 'offline' }, 'no route to host'),
      target('did_p6q7r8s9t0', null, { tag: 'pending' }),
    ],
  }
}

describe('EntryDeliveryBadge', () => {
  it('renders nothing when delivery is null', () => {
    const { container } = render(<EntryDeliveryBadge delivery={null} />)
    expect(container).toBeEmptyDOMElement()
  })

  it('renders local source + partial summary for mixed delivery', () => {
    render(<EntryDeliveryBadge delivery={mixedDelivery()} />)

    // Source 段(本机): localShort 文案
    expect(screen.getByText(i18n.t('delivery.source.localShort'))).toBeInTheDocument()

    // Summary 段: 有 failed → partial
    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    expect(trigger).toHaveAttribute('data-summary', 'partial')
    expect(trigger).toHaveTextContent(i18n.t('delivery.summary.partial'))
  })

  it('renders historical source without summary chip', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-old',
      source: { tag: 'historical' },
      deliveries: [],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    expect(screen.getByText(i18n.t('delivery.source.historicalShort'))).toBeInTheDocument()
    // Historical 来源不渲染 summary 段,popover trigger 应不存在
    expect(
      screen.queryByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    ).not.toBeInTheDocument()
  })

  it('hides summary chip for local entry without trusted peers', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-solo',
      source: { tag: 'local' },
      deliveries: [],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    expect(screen.getByText(i18n.t('delivery.source.localShort'))).toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    ).not.toBeInTheDocument()
  })

  it('renders remote source with truncated device id when name is missing', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-remote',
      source: { tag: 'remote', deviceId: 'did_sender_xyz', deviceName: null },
      deliveries: [target('did_peer_aaa', null, { tag: 'delivered' })],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    // 名字缺失 → fallback 到 device_id 截断(前 8 字符 + …)
    expect(
      screen.getByText(i18n.t('delivery.source.remoteShort', { device: 'did_send…' }))
    ).toBeInTheDocument()
  })

  it('prefers device names over device ids when resolved', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-named',
      source: { tag: 'remote', deviceId: 'did_sender_xyz', deviceName: 'Mac Studio' },
      deliveries: [target('did_target_aaa', 'iPad Pro', { tag: 'delivered' })],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    // Source 用真实 name 而不是截断 id
    expect(
      screen.getByText(i18n.t('delivery.source.remoteShort', { device: 'Mac Studio' }))
    ).toBeInTheDocument()
  })

  it.each<[Exclude<EntryDeliveryView['deliveries'][number]['status']['tag'], 'failed'>, string]>([
    ['delivered', 'synced'],
    ['duplicate', 'synced'],
    ['pending', 'pending'],
  ])('summarizes single %s peer as %s', (statusTag, summary) => {
    const delivery: EntryDeliveryView = {
      entryId: `entry-${statusTag}`,
      source: { tag: 'local' },
      deliveries: [
        target('did_peer_aaaaaa', null, {
          tag: statusTag,
        } as EntryDeliveryTargetView['status']),
      ],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    expect(trigger).toHaveAttribute('data-summary', summary)
  })

  it('summarizes all-failed peers as failed', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-failed-all',
      source: { tag: 'local' },
      deliveries: [
        target('did_a', null, { tag: 'failed', reason: 'offline' }),
        target('did_b', null, { tag: 'failed', reason: 'io' }),
      ],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    expect(trigger).toHaveAttribute('data-summary', 'failed')
    expect(trigger).toHaveTextContent(i18n.t('delivery.summary.failed'))
  })

  it('summarizes delivered + pending as syncing', () => {
    const delivery: EntryDeliveryView = {
      entryId: 'entry-syncing',
      source: { tag: 'local' },
      deliveries: [
        target('did_a', null, { tag: 'delivered' }),
        target('did_b', null, { tag: 'pending' }),
      ],
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    expect(trigger).toHaveAttribute('data-summary', 'syncing')
    expect(trigger).toHaveTextContent(i18n.t('delivery.summary.syncing'))
  })

  it('reveals popover content with all peer rows on hover', async () => {
    const user = userEvent.setup()
    render(<EntryDeliveryBadge delivery={mixedDelivery()} />)

    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    await user.hover(trigger)

    // popover 标题 + 四档状态文案均在 popover 中可见
    expect(await screen.findByText(i18n.t('delivery.popover.title'))).toBeInTheDocument()
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

    // 截断后的 device id 渲染
    expect(screen.getByText('did_a1b2…')).toBeInTheDocument()
    expect(screen.getByText('did_f6g7…')).toBeInTheDocument()
    expect(screen.getByText('did_k1l2…')).toBeInTheDocument()
    expect(screen.getByText('did_p6q7…')).toBeInTheDocument()
  })

  describe('resend integration', () => {
    beforeEach(() => {
      resendEntryMock.mockReset()
      toastSuccessMock.mockReset()
      toastErrorMock.mockReset()
      // 模块级 in-flight store 跨 case 共享,case 间显式清空避免渗漏。
      __resetResendActionStoreForTests()
    })

    afterEach(() => {
      resendEntryMock.mockReset()
      __resetResendActionStoreForTests()
    })

    const successReport: ResendEntryReportDto = {
      accepted: 1,
      duplicate: 0,
      offline: 0,
      errored: 0,
      pending: 0,
    }

    function deliveryWithFailingPeer(): EntryDeliveryView {
      return {
        entryId: 'entry-resend-1',
        source: { tag: 'local' },
        deliveries: [
          target('did_ok_aaaaaa', 'Mac', { tag: 'delivered' }),
          target('did_failed_bbbb', 'iPad', { tag: 'failed', reason: 'offline' }, 'unreachable'),
        ],
      }
    }

    it('shows entry-level Resend button only for local entries with eligible peers', async () => {
      const user = userEvent.setup()
      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)

      const trigger = screen.getByRole('button', {
        name: i18n.t('delivery.popover.ariaTrigger'),
      })
      await user.hover(trigger)

      const resendButton = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.entryAria'),
      })
      expect(resendButton).not.toBeDisabled()
    })

    it('hides Resend UI entirely for remote-origin entries', async () => {
      const user = userEvent.setup()
      const delivery: EntryDeliveryView = {
        entryId: 'entry-from-other',
        source: { tag: 'remote', deviceId: 'did_other_xyz', deviceName: 'Other' },
        deliveries: [target('did_local_self', 'Self', { tag: 'failed', reason: 'offline' })],
      }
      render(<EntryDeliveryBadge delivery={delivery} />)

      const trigger = screen.getByRole('button', {
        name: i18n.t('delivery.popover.ariaTrigger'),
      })
      await user.hover(trigger)
      // popover 应该挂载,但 entry-level / peer-level 重发按钮都不应渲染。
      await screen.findByText(i18n.t('delivery.popover.title'))
      expect(
        screen.queryByRole('button', { name: i18n.t('delivery.resend.button.entryAria') })
      ).not.toBeInTheDocument()
      expect(
        screen.queryByRole('button', {
          name: i18n.t('delivery.resend.button.peerAria', { device: 'Self' }),
        })
      ).not.toBeInTheDocument()
    })

    it('disables entry-level Resend when every peer is already delivered/duplicate', async () => {
      const user = userEvent.setup()
      const delivery: EntryDeliveryView = {
        entryId: 'entry-all-good',
        source: { tag: 'local' },
        deliveries: [
          target('did_a_aaaa', 'A', { tag: 'delivered' }),
          target('did_b_bbbb', 'B', { tag: 'duplicate' }),
        ],
      }
      render(<EntryDeliveryBadge delivery={delivery} />)

      const trigger = screen.getByRole('button', {
        name: i18n.t('delivery.popover.ariaTrigger'),
      })
      await user.hover(trigger)
      const button = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.entryDisabled'),
      })
      expect(button).toBeDisabled()
    })

    it('renders per-peer Resend only for failed/pending peers', async () => {
      const user = userEvent.setup()
      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)

      const trigger = screen.getByRole('button', {
        name: i18n.t('delivery.popover.ariaTrigger'),
      })
      await user.hover(trigger)

      // Failed peer 有按钮
      expect(
        await screen.findByRole('button', {
          name: i18n.t('delivery.resend.button.peerAria', { device: 'iPad' }),
        })
      ).toBeInTheDocument()
      // Delivered peer 没有按钮
      expect(
        screen.queryByRole('button', {
          name: i18n.t('delivery.resend.button.peerAria', { device: 'Mac' }),
        })
      ).not.toBeInTheDocument()
    })

    it('triggers entry-wide resend and shows success toast with accepted/total ratio', async () => {
      const user = userEvent.setup()
      resendEntryMock.mockResolvedValueOnce({
        accepted: 1,
        duplicate: 0,
        offline: 1,
        errored: 0,
        pending: 0,
      } satisfies ResendEntryReportDto)

      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)

      await user.hover(screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') }))
      const button = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.entryAria'),
      })
      await user.click(button)

      await waitFor(() => {
        expect(resendEntryMock).toHaveBeenCalledWith({
          entryId: 'entry-resend-1',
          targetDeviceIds: null,
        })
      })
      await waitFor(() => {
        expect(toastSuccessMock).toHaveBeenCalledWith(
          i18n.t('delivery.resend.success.summary', { accepted: 1, total: 2 })
        )
      })
      expect(toastErrorMock).not.toHaveBeenCalled()
    })

    it('peer-level button passes a single-device filter and surfaces success', async () => {
      const user = userEvent.setup()
      resendEntryMock.mockResolvedValueOnce(successReport)
      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)

      await user.hover(screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') }))
      const button = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.peerAria', { device: 'iPad' }),
      })
      await user.click(button)

      await waitFor(() => {
        expect(resendEntryMock).toHaveBeenCalledWith({
          entryId: 'entry-resend-1',
          targetDeviceIds: ['did_failed_bbbb'],
        })
      })
      await waitFor(() => {
        expect(toastSuccessMock).toHaveBeenCalled()
      })
    })

    it('translates typed ResendEntryCommandError variants into i18n strings (with entryId placeholder)', async () => {
      const user = userEvent.setup()
      resendEntryMock.mockRejectedValueOnce({
        code: 'ENTRY_NOT_RESENDABLE',
        // entryId 走 typed envelope (commit G:加入 entry_id 字段),用户在
        // toast 上能看到截断 id —— 多条历史同时报错时不会一头雾水。
        entryId: 'entry-payload-lost-uuid',
        reason: 'payloadLost',
      })

      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)
      await user.hover(screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') }))
      const button = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.entryAria'),
      })
      await user.click(button)

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalledWith(
          i18n.t('delivery.resend.error.notResendable.payloadLost', {
            entryIdShort: 'entry-pa…',
          })
        )
      })
      expect(toastSuccessMock).not.toHaveBeenCalled()
    })

    it('falls back to internal error message for unknown rejections', async () => {
      const user = userEvent.setup()
      resendEntryMock.mockRejectedValueOnce(new Error('boom'))

      render(<EntryDeliveryBadge delivery={deliveryWithFailingPeer()} />)
      await user.hover(screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') }))
      const button = await screen.findByRole('button', {
        name: i18n.t('delivery.resend.button.entryAria'),
      })
      await user.click(button)

      await waitFor(() => {
        expect(toastErrorMock).toHaveBeenCalledWith(
          i18n.t('delivery.resend.error.internal', { message: 'boom' })
        )
      })
    })
  })

  it('maps all five failure reasons to their i18n labels in popover', async () => {
    const user = userEvent.setup()
    const reasons: ReadonlyArray<DeliveryFailureReason> = [
      'offline',
      'localPolicy',
      'peerRejected',
      'io',
      'internal',
    ]
    const delivery: EntryDeliveryView = {
      entryId: 'entry-failures',
      source: { tag: 'local' },
      deliveries: reasons.map((reason, idx) =>
        target(`did_peer_${idx}xxxxxxxx`, null, { tag: 'failed', reason })
      ),
    }
    render(<EntryDeliveryBadge delivery={delivery} />)

    const trigger = screen.getByRole('button', { name: i18n.t('delivery.popover.ariaTrigger') })
    await user.hover(trigger)

    // 等待 popover 挂载 (HoverCard 异步 mount)
    await screen.findByText(i18n.t('delivery.popover.title'))

    for (const reason of reasons) {
      const label = i18n.t('delivery.status.failedWithReason', {
        reason: i18n.t(`delivery.failureReason.${reason}`),
      })
      expect(screen.getByText(label)).toBeInTheDocument()
    }
  })
})
