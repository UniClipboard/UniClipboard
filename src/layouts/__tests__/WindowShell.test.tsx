import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { WindowShell } from '@/layouts/WindowShell'

const windowFrame = vi.hoisted(() => ({
  hasRoundedWindow: true,
}))

vi.mock('@/hooks/useWindowFrame', () => ({
  useWindowFrame: () => ({ hasRoundedWindow: windowFrame.hasRoundedWindow }),
}))

describe('WindowShell', () => {
  beforeEach(() => {
    windowFrame.hasRoundedWindow = true
  })

  it('clips the custom frame to rounded window corners', () => {
    const { container } = render(<WindowShell titleBar={<header>title</header>}>body</WindowShell>)

    expect(container.firstElementChild).toHaveClass('rounded-xl')
  })

  it('leaves corner shape to the system frame', () => {
    windowFrame.hasRoundedWindow = false

    const { container } = render(<WindowShell titleBar={null}>body</WindowShell>)

    expect(container.firstElementChild).not.toHaveClass('rounded-xl')
  })

  it('does not render the circular window background accent', () => {
    const { container } = render(<WindowShell titleBar={null}>body</WindowShell>)

    expect(container.querySelector('[data-uc-decorative-effect].rounded-full')).toBeNull()
  })
})
