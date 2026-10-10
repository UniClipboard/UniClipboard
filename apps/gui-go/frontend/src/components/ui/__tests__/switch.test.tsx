import { fireEvent, render, screen } from '@testing-library/react'
import React from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Switch } from '@/components/ui/switch'

const soundMocks = vi.hoisted(() => ({
  playUiSound: vi.fn(),
}))

vi.mock('@/lib/ui-sound', () => soundMocks)

vi.mock('framer-motion', async () => {
  const ReactModule = await import('react')

  return {
    MotionConfig: ({ children }: { children: React.ReactNode }) => children,
    useReducedMotion: () => false,
    m: {
      button: ({
        children,
        initial: _initial,
        ...props
      }: React.ButtonHTMLAttributes<HTMLButtonElement> & { initial?: unknown }) =>
        ReactModule.createElement('button', props, children),
      div: ({
        animate,
        initial,
        layout,
        ...props
      }: React.HTMLAttributes<HTMLDivElement> & {
        animate?: { x?: number; y?: number; scale?: number }
        initial?: unknown
        layout?: unknown
      }) =>
        ReactModule.createElement('div', {
          ...props,
          'data-motion-x': animate?.x,
          'data-motion-y': animate?.y,
          'data-motion-scale': animate?.scale,
          'data-motion-initial': String(initial),
          'data-motion-layout': layout == null ? undefined : String(layout),
        }),
    },
  }
})

describe('Switch', () => {
  beforeEach(() => {
    soundMocks.playUiSound.mockClear()
  })

  it('plays toggle feedback for an accepted click', () => {
    const onCheckedChange = vi.fn()
    render(<Switch checked={false} onCheckedChange={onCheckedChange} aria-label="Sync" />)

    fireEvent.click(screen.getByRole('switch', { name: 'Sync' }))

    expect(soundMocks.playUiSound).toHaveBeenCalledOnce()
    expect(soundMocks.playUiSound).toHaveBeenCalledWith('toggle')
  })

  it('does not play toggle feedback when disabled', () => {
    render(<Switch checked={false} disabled onCheckedChange={vi.fn()} aria-label="Sync" />)

    fireEvent.click(screen.getByRole('switch', { name: 'Sync' }))

    expect(soundMocks.playUiSound).not.toHaveBeenCalled()
  })

  it('keeps the unchecked track visible on dark backgrounds', () => {
    render(<Switch checked={false} aria-label="Sync" />)

    expect(screen.getByRole('switch', { name: 'Sync' })).toHaveClass('dark:bg-foreground/20')
  })

  it('uses the contrasting theme color for the checked thumb', () => {
    render(<Switch checked aria-label="Sync" />)

    expect(screen.getByRole('switch', { name: 'Sync' }).firstElementChild).toHaveClass(
      'bg-primary-foreground'
    )
  })

  it('limits thumb motion to the horizontal toggle axis', () => {
    const onCheckedChange = vi.fn()
    const { rerender } = render(
      <Switch checked={false} onCheckedChange={onCheckedChange} aria-label="Sync" />
    )
    const control = screen.getByRole('switch', { name: 'Sync' })
    const thumb = control.firstElementChild

    expect(thumb).toHaveAttribute('data-motion-x', '0')
    expect(thumb).not.toHaveAttribute('data-motion-y')
    expect(thumb).not.toHaveAttribute('data-motion-layout')
    expect(thumb).toHaveAttribute('data-motion-initial', 'false')

    fireEvent.click(control)
    expect(onCheckedChange).toHaveBeenCalledWith(true)

    rerender(<Switch checked onCheckedChange={onCheckedChange} aria-label="Sync" />)
    expect(thumb).toHaveAttribute('data-motion-x', '12')
  })
})
