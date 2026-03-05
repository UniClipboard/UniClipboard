import { render, screen } from '@testing-library/react'
import type { HTMLAttributes, ReactNode } from 'react'
import { describe, expect, it, vi } from 'vitest'
import StepLayout from '@/pages/setup/StepLayout'

vi.mock('framer-motion', () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: new Proxy(
    {},
    {
      get: () => (props: HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('StepLayout', () => {
  it('renders title text', () => {
    render(<StepLayout title="Test Title" />)
    expect(screen.getByText('Test Title')).toBeInTheDocument()
  })

  it('renders subtitle when provided', () => {
    render(<StepLayout title="Title" subtitle="Test Subtitle" />)
    expect(screen.getByText('Test Subtitle')).toBeInTheDocument()
  })

  it('renders children in content area', () => {
    render(
      <StepLayout title="Title">
        <p>Content Here</p>
      </StepLayout>
    )
    expect(screen.getByText('Content Here')).toBeInTheDocument()
  })

  it('renders footer when provided', () => {
    render(<StepLayout title="Title" footer={<button>Next</button>} />)
    expect(screen.getByText('Next')).toBeInTheDocument()
  })

  it('renders hint text when provided', () => {
    render(<StepLayout title="Title" hint="Some hint" />)
    expect(screen.getByText('Some hint')).toBeInTheDocument()
  })

  it('renders error with AlertCircle icon when error string is provided', () => {
    const { container } = render(<StepLayout title="Title" error="Something went wrong" />)
    expect(screen.getByText('Something went wrong')).toBeInTheDocument()
    // AlertCircle renders as an svg
    const svg = container.querySelector('svg')
    expect(svg).toBeTruthy()
  })

  it('does not render error section when error is null', () => {
    render(<StepLayout title="Title" error={null} />)
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('does not render error section when error is undefined', () => {
    render(<StepLayout title="Title" />)
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('renders headerLeft and headerRight in header row', () => {
    render(
      <StepLayout title="Title" headerLeft={<span>Left</span>} headerRight={<span>Right</span>} />
    )
    expect(screen.getByText('Left')).toBeInTheDocument()
    expect(screen.getByText('Right')).toBeInTheDocument()
  })

  it('does not render header row when both headerLeft and headerRight are absent', () => {
    const { container } = render(<StepLayout title="Title" />)
    expect(container.querySelector('[data-testid="step-header"]')).toBeNull()
  })

  it('variant centered adds text-center to title section and justify-center to footer', () => {
    const { container } = render(
      <StepLayout title="Title" variant="centered" footer={<button>Go</button>} />
    )
    const titleSection = container.querySelector('[data-testid="step-title-section"]')
    expect(titleSection).toHaveClass('text-center')
    const footerSection = container.querySelector('[data-testid="step-footer"]')
    expect(footerSection).toHaveClass('justify-center')
  })

  it('wraps content in a div for animation (motion.div mocked)', () => {
    const { container } = render(
      <StepLayout title="Title">
        <p>Animated</p>
      </StepLayout>
    )
    expect(container.querySelector('div')).toBeTruthy()
    expect(screen.getByText('Animated')).toBeInTheDocument()
  })
})
