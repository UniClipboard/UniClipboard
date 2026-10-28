import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it } from 'vitest'
import { LanOnlyDisclosure } from '@/components/setting/LanOnlyDisclosure'
import i18n from '@/i18n'

beforeAll(async () => {
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  cleanup()
})

describe('LanOnlyDisclosure', () => {
  it('Test 1: trigger 是 <button> 含 aria-haspopup="dialog"', () => {
    render(<LanOnlyDisclosure />)
    const trigger = screen.getByRole('button', {
      name: /查看 LAN-only|View the list/,
    })
    expect(trigger).toBeInTheDocument()
    expect(trigger.tagName.toLowerCase()).toBe('button')
    expect(trigger).toHaveAttribute('aria-haspopup', 'dialog')
  })

  it('Test 2: trigger 含 lucide Info icon (svg, aria-hidden)', () => {
    const { container } = render(<LanOnlyDisclosure />)
    const trigger = screen.getByRole('button', {
      name: /查看 LAN-only|View the list/,
    })
    const svg = trigger.querySelector('svg')
    expect(svg).not.toBeNull()
    // 装饰性 icon 应有 aria-hidden（lucide 默认不一定加，由组件代码显式补）
    expect(svg).toHaveAttribute('aria-hidden', 'true')
    // 兜底：container 内确实有 svg 节点
    expect(container.querySelector('svg')).not.toBeNull()
  })

  it('Test 3: hover 不打开 Popover — D-C1 click-only fence', () => {
    render(<LanOnlyDisclosure />)
    const trigger = screen.getByRole('button', {
      name: /查看 LAN-only|View the list/,
    })
    fireEvent.mouseEnter(trigger)
    fireEvent.mouseOver(trigger)
    // hover 不应打开 dialog（Base UI Popover 默认就 click-only — 此测试是 fence）
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('Test 4: click 打开 Popover (role=dialog)', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    const trigger = screen.getByRole('button', {
      name: /查看 LAN-only|View the list/,
    })
    await user.click(trigger)
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('describes local pairing alongside telemetry and updates', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(
      screen.getByText(/无需公共 rendezvous 的配对|Pairing without public rendezvous/)
    ).toBeInTheDocument()
    expect(screen.getByText(/^遥测$|^Telemetry$/)).toBeInTheDocument()
    expect(screen.getByText(/本地设备发现|Local device discovery/)).toBeInTheDocument()
    expect(screen.getByText(/六位短码仅通过本地网络发现解析/)).toHaveTextContent('完整邀请')
    expect(screen.getByText(/DNS\/pkarr 查询已禁用/)).toBeInTheDocument()
    expect(screen.getByText(/自动更新 GitHub 检查|Auto-update GitHub check/)).toBeInTheDocument()
  })

  it('Test 6: Esc 关闭 Popover', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await user.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('explains the disabled connection services', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(
      screen.getByText(/LAN-only 配对与互联网请求|LAN-only pairing and internet requests/)
    ).toBeInTheDocument()
    expect(
      screen.getByText(/禁用中继服务器、公共 rendezvous 和 DNS\/pkarr|disables relay servers/)
    ).toBeInTheDocument()
  })
})
