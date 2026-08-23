import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import MainLayout from '../MainLayout'

const platformState = vi.hoisted(() => ({
  current: {
    isWindows: false,
    isMac: false,
    isLinux: false,
    isTauri: false,
    reduceVisualEffects: false,
  },
}))

const windowFrameState = vi.hoisted(() => ({
  useSystemWindowFrame: false,
}))

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: () => platformState.current,
}))

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: () => windowFrameState,
}))

vi.mock('@/contexts/titlebar-slot-context', () => ({
  useTitleBarSlot: () => ({ rightSlotHost: null }),
}))

vi.mock('@/components', () => ({
  Sidebar: ({ className }: { className?: string }) => (
    <aside data-testid="sidebar" className={className} />
  ),
}))

const renderLayout = () =>
  render(
    <MemoryRouter>
      <MainLayout>
        <div data-testid="content" />
      </MainLayout>
    </MemoryRouter>
  )

describe('MainLayout', () => {
  it('Linux 自绘窗口框使用与标题栏一致的内嵌布局', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
      reduceVisualEffects: true,
    }
    windowFrameState.useSystemWindowFrame = false

    const { container } = renderLayout()
    const main = container.querySelector('main')
    const inset = main?.querySelector('.pb-2.pr-2')

    expect(inset).toBeInTheDocument()
    expect(inset?.firstElementChild).toHaveClass('rounded-xl')
  })

  it('Linux 系统窗口框使用平面布局', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
      reduceVisualEffects: true,
    }
    windowFrameState.useSystemWindowFrame = true

    const { container } = renderLayout()
    const main = container.querySelector('main')

    expect(main).toHaveClass('bg-card')
    expect(main?.querySelector('.pb-2.pr-2')).not.toBeInTheDocument()
    expect(main?.querySelector('.rounded-xl')).not.toBeInTheDocument()
  })

  it('侧栏固定为窄栏并提供历史与设备两个页面入口', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: false,
      isTauri: false,
      reduceVisualEffects: false,
    }
    windowFrameState.useSystemWindowFrame = false

    const { container } = renderLayout()
    const sidebar = container.querySelector('aside')

    expect(sidebar).toHaveClass('w-12')
    expect(screen.getByRole('link', { name: 'History' })).toHaveAttribute('href', '/history')
    expect(screen.getByRole('link', { name: 'Devices' })).toHaveAttribute('href', '/devices')
    expect(screen.queryByRole('button', { name: /sidebar/i })).not.toBeInTheDocument()
  })
})
