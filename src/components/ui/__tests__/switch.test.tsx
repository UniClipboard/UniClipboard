import { fireEvent, render, screen } from '@testing-library/react'
import React from 'react'
import { describe, expect, it, vi } from 'vitest'
import { Switch } from '@/components/ui/switch'

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
