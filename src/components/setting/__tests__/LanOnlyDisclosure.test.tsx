import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
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
    // hover 不应打开 dialog（Radix Popover 默认就 click-only — 此测试是 fence）
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

  it('Test 5: PopoverContent 含 4 类外网请求标题', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    // 4 类清单标题（zh-CN 锁定 — i18n 已切到 zh-CN）
    expect(screen.getByText(/首次配对 rendezvous|First-pairing rendezvous/)).toBeInTheDocument()
    expect(screen.getByText(/OTLP 遥测|OTLP telemetry/)).toBeInTheDocument()
    expect(
      screen.getByText(/pkarr DHT NodeId 解析|pkarr DHT NodeId resolution/)
    ).toBeInTheDocument()
    expect(screen.getByText(/自动更新 GitHub 检查|Auto-update GitHub check/)).toBeInTheDocument()
  })

  it('Test 6: Esc 关闭 Popover', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('Test 7: Popover 标题 + intro 文案双语命中', async () => {
    const user = userEvent.setup()
    render(<LanOnlyDisclosure />)
    await user.click(screen.getByRole('button', { name: /查看 LAN-only|View the list/ }))
    expect(
      screen.getByText(/LAN-only 开启后仍会走外网的请求|Requests that still reach the internet/)
    ).toBeInTheDocument()
    expect(
      screen.getByText(
        /以下 4 类请求由独立模块控制|The following 4 request types are controlled by independent modules/
      )
    ).toBeInTheDocument()
  })
})
