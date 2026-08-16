import { render, screen } from '@testing-library/react'
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

vi.mock('@/components', () => ({
  Sidebar: ({ className }: { className?: string }) => (
    <aside data-testid="sidebar" className={className} />
  ),
}))

const renderLayout = () =>
  render(
    <MainLayout>
      <div data-testid="content" />
    </MainLayout>
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

    expect(main).toHaveClass('pb-2')
    expect(main).toHaveClass('pr-2')
    expect(container.innerHTML).toContain('rounded-[1.25rem]')
    expect(screen.getByTestId('sidebar')).not.toHaveClass('border-r')
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
    expect(main).not.toHaveClass('pb-2')
    expect(main).not.toHaveClass('pr-2')
    expect(container.innerHTML).not.toContain('rounded-[1.25rem]')
    expect(screen.getByTestId('sidebar')).toHaveClass('border-r')
  })
})
