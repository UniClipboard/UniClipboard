import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { WindowShell } from '@/layouts/WindowShell'

describe('WindowShell', () => {
  it('fills the native window without cutting transparent corners', () => {
    const { container } = render(<WindowShell titleBar={<header>title</header>}>body</WindowShell>)

    expect(container.firstElementChild).not.toHaveClass('rounded-xl')
  })

  it('leaves corner shape to the system frame', () => {
    const { container } = render(<WindowShell titleBar={null}>body</WindowShell>)

    expect(container.firstElementChild).not.toHaveClass('rounded-xl')
  })

  it('does not render the circular window background accent', () => {
    const { container } = render(<WindowShell titleBar={null}>body</WindowShell>)

    expect(container.querySelector('[data-uc-decorative-effect].rounded-full')).toBeNull()
  })
})
