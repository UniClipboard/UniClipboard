import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import StepDotIndicator from '@/pages/setup/StepDotIndicator'

describe('StepDotIndicator', () => {
  it('renders correct number of dots matching totalSteps', () => {
    const { container } = render(<StepDotIndicator totalSteps={5} currentStep={0} />)
    const dots = container.querySelectorAll('[data-testid^="dot-"]')
    expect(dots).toHaveLength(5)
  })

  it('fills the dot at currentStep index', () => {
    const { container } = render(<StepDotIndicator totalSteps={4} currentStep={2} />)
    const activeDot = container.querySelector('[data-testid="dot-2"]')
    expect(activeDot).toHaveClass('bg-primary')
  })

  it('other dots are hollow (border only)', () => {
    const { container } = render(<StepDotIndicator totalSteps={3} currentStep={1} />)
    const dot0 = container.querySelector('[data-testid="dot-0"]')
    const dot2 = container.querySelector('[data-testid="dot-2"]')
    expect(dot0).not.toHaveClass('bg-primary')
    expect(dot0).toHaveClass('border')
    expect(dot2).not.toHaveClass('bg-primary')
    expect(dot2).toHaveClass('border')
  })
})
