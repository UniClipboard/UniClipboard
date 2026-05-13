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

vi.mock('@/hooks/usePlatform', () => ({
  usePlatform: () => platformState.current,
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
  it('Linux Tauri 使用无圆角主内容布局', () => {
    platformState.current = {
      isWindows: false,
      isMac: false,
      isLinux: true,
      isTauri: true,
      reduceVisualEffects: true,
    }

    const { container } = renderLayout()
    const main = container.querySelector('main')

    expect(main).toHaveClass('bg-card')
    expect(main).not.toHaveClass('pb-2')
    expect(main).not.toHaveClass('pr-2')
    expect(container.innerHTML).not.toContain('rounded-[1.25rem]')
    expect(screen.getByTestId('sidebar')).toHaveClass('border-r')
  })

  it('非 Linux Tauri 保持内嵌圆角布局', () => {
    platformState.current = {
      isWindows: true,
      isMac: false,
      isLinux: false,
      isTauri: true,
      reduceVisualEffects: false,
    }

    const { container } = renderLayout()
    const main = container.querySelector('main')

    expect(main).toHaveClass('pb-2')
    expect(main).toHaveClass('pr-2')
    expect(container.innerHTML).toContain('rounded-[1.25rem]')
    expect(screen.getByTestId('sidebar').className).not.toContain('border-r')
  })
})
