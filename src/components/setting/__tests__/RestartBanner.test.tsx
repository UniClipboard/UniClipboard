import '@testing-library/jest-dom/vitest'
import { render, screen, cleanup } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { RestartBanner } from '@/components/setting/RestartBanner'
import i18n from '@/i18n'

beforeAll(async () => {
  // 测试默认锁中文（zh-CN），让正则可双语兼容（包含 zh 关键字）
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  cleanup()
})

const MSG = '需要重启应用以使更改生效。'

describe('RestartBanner', () => {
  it('Test 1: visible=false 时不渲染（节点不挂）', () => {
    const { container } = render(
      <RestartBanner
        visible={false}
        message={MSG}
        onRestart={vi.fn().mockResolvedValue(undefined)}
      />
    )
    expect(screen.queryByRole('status')).toBeNull()
    // 整个组件应返回 null — container 内无任何子元素
    expect(container.firstChild).toBeNull()
  })

  it('Test 2: visible=true 时渲染主信息 + role=status + aria-live="polite"', () => {
    render(<RestartBanner visible message={MSG} onRestart={vi.fn().mockResolvedValue(undefined)} />)
    const status = screen.getByRole('status')
    expect(status).toBeInTheDocument()
    expect(status).toHaveAttribute('aria-live', 'polite')
    // 主信息文案由调用方注入，banner 原样渲染
    expect(status.textContent).toContain(MSG)
  })

  it('Test 3: 点击「立即重启」Button 触发 onRestart 一次', async () => {
    const onRestart = vi.fn().mockResolvedValue(undefined)
    const user = userEvent.setup()
    render(<RestartBanner visible message={MSG} onRestart={onRestart} />)
    const button = screen.getByRole('button', { name: /立即重启|Restart now/ })
    await user.click(button)
    expect(onRestart).toHaveBeenCalledTimes(1)
  })

  it('Test 4: loading=true 时 Button 禁用 + 文案变 "正在重启…"', () => {
    render(
      <RestartBanner
        visible
        loading
        message={MSG}
        onRestart={vi.fn().mockResolvedValue(undefined)}
      />
    )
    const button = screen.getByRole('button', { name: /正在重启|Restarting/ })
    expect(button).toBeDisabled()
  })

  it('Test 5: error 不为 null 时显示 role=alert 错误文本', () => {
    render(
      <RestartBanner
        visible
        message={MSG}
        error="自动重启失败"
        onRestart={vi.fn().mockResolvedValue(undefined)}
      />
    )
    const alert = screen.getByRole('alert')
    expect(alert).toBeInTheDocument()
    expect(alert).toHaveTextContent('自动重启失败')
  })

  it('Test 6: error 状态显示「重试」Button 与 dismiss X icon；点击 dismiss 触发 onDismissError', async () => {
    const onRestart = vi.fn().mockResolvedValue(undefined)
    const onDismissError = vi.fn()
    const user = userEvent.setup()
    render(
      <RestartBanner
        visible
        message={MSG}
        error="自动重启失败"
        onRestart={onRestart}
        onDismissError={onDismissError}
      />
    )
    // 「重试」按钮存在 + 点击触发 onRestart
    const retryBtn = screen.getByRole('button', { name: /重试|Retry/ })
    expect(retryBtn).toBeInTheDocument()
    await user.click(retryBtn)
    expect(onRestart).toHaveBeenCalledTimes(1)
    // dismiss X icon button 通过 aria-label 找到
    const dismissBtn = screen.getByRole('button', { name: /收起重启提示|Dismiss restart notice/ })
    expect(dismissBtn).toBeInTheDocument()
    await user.click(dismissBtn)
    expect(onDismissError).toHaveBeenCalledTimes(1)
  })

  it('Test 7: visible=true 时含 lucide RefreshCw icon (svg)', () => {
    const { container } = render(
      <RestartBanner visible message={MSG} onRestart={vi.fn().mockResolvedValue(undefined)} />
    )
    // lucide-react 输出 <svg class="lucide lucide-refresh-cw ..."> — 用 svg 选择器即可
    const svg = container.querySelector('svg')
    expect(svg).not.toBeNull()
  })

  it('Test 8: 不复用 shadcn Alert — 静态结构 fence (data-slot="alert" 不应存在)', () => {
    const { container } = render(
      <RestartBanner visible message={MSG} onRestart={vi.fn().mockResolvedValue(undefined)} />
    )
    expect(container.querySelector('[data-slot="alert"]')).toBeNull()
  })
})
