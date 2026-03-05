import { render, screen, fireEvent } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'
import ProcessingJoinStep from '@/pages/setup/ProcessingJoinStep'

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('ProcessingJoinStep', () => {
  it('renders title text from i18n', () => {
    render(<ProcessingJoinStep onCancel={() => {}} />)
    const heading = screen.getByRole('heading')
    expect(heading).toBeInTheDocument()
  })

  it('renders subtitle text', () => {
    render(<ProcessingJoinStep onCancel={() => {}} />)
    const subtitle = screen.getByText((_content, element) => {
      return (
        (element?.tagName === 'P' && element?.classList.contains('text-muted-foreground')) || false
      )
    })
    expect(subtitle).toBeInTheDocument()
  })

  it('renders a cancel button', () => {
    render(<ProcessingJoinStep onCancel={() => {}} />)
    const cancelBtn = screen.getByRole('button')
    expect(cancelBtn).toBeInTheDocument()
  })

  it('calls onCancel when cancel button clicked', () => {
    const onCancel = vi.fn()
    render(<ProcessingJoinStep onCancel={onCancel} />)
    const cancelBtn = screen.getByRole('button')
    fireEvent.click(cancelBtn)
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('shows loading spinner', () => {
    const { container } = render(<ProcessingJoinStep onCancel={() => {}} />)
    const spinner = container.querySelector('.animate-spin')
    expect(spinner).toBeTruthy()
  })
})
