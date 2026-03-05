import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import WelcomeStep from '@/pages/setup/WelcomeStep'

vi.mock('framer-motion', () => ({
  motion: new Proxy(
    {},
    {
      get: () => (props: React.HTMLAttributes<HTMLDivElement>) => <div {...props} />,
    }
  ),
}))

describe('WelcomeStep layout', () => {
  it('uses fixed two-column cards layout', () => {
    const { container } = render(
      <WelcomeStep onCreate={() => {}} onJoin={() => {}} loading={false} />
    )

    screen.getByRole('heading', { name: /create/i })
    const cardsGrid = container.querySelector('div.grid.gap-4')
    expect(cardsGrid).toBeTruthy()
    expect(cardsGrid).toHaveClass('grid-cols-2')
  })
})
