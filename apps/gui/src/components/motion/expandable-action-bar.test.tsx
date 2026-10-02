import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { LazyMotion, domMax } from 'framer-motion'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ExpandableActionBar } from './expandable-action-bar'

vi.mock('@/hooks/useVisualEffects', () => ({ useReducedMotion: () => true }))

afterEach(cleanup)

function tapAction() {
  const button = screen.getByRole('button', { name: 'Copy' })
  fireEvent.pointerDown(button, { pointerType: 'touch' })
  fireEvent.click(button, { detail: 1 })
}

describe('ExpandableActionBar touch activation', () => {
  it('reveals labels on the first tap and acts on the second', () => {
    const onClick = vi.fn()
    render(
      <LazyMotion features={domMax}>
        <ExpandableActionBar items={[{ id: 'copy', label: 'Copy', icon: null, onClick }]} />
      </LazyMotion>
    )

    tapAction()
    expect(onClick).not.toHaveBeenCalled()
    expect(screen.getByText('Copy')).toHaveAttribute('aria-hidden', 'false')
    tapAction()
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('allows the second tap when the parent declines expansion', () => {
    const onClick = vi.fn()
    render(
      <LazyMotion features={domMax}>
        <ExpandableActionBar
          expanded={false}
          items={[{ id: 'copy', label: 'Copy', icon: null, onClick }]}
        />
      </LazyMotion>
    )

    tapAction()
    expect(onClick).not.toHaveBeenCalled()
    tapAction()
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('requires a fresh reveal tap after the parent collapses the bar', () => {
    const onClick = vi.fn()
    const items = [{ id: 'copy', label: 'Copy', icon: null, onClick }]
    const view = (expanded: boolean) => (
      <LazyMotion features={domMax}>
        <ExpandableActionBar expanded={expanded} items={items} />
      </LazyMotion>
    )
    const { rerender } = render(view(false))

    tapAction()
    rerender(view(true))
    rerender(view(false))
    expect(screen.getByText('Copy')).toHaveAttribute('aria-hidden', 'true')
    tapAction()
    expect(onClick).not.toHaveBeenCalled()
    tapAction()
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
