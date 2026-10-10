import { act, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useInView } from '@/hooks/useInView'

function Probe() {
  const [ref, inView] = useInView<HTMLDivElement>()
  return (
    <div ref={ref} data-testid="probe">
      {String(inView)}
    </div>
  )
}

describe('useInView', () => {
  let observedCallback: IntersectionObserverCallback | null = null
  let observeSpy: ReturnType<typeof vi.fn>
  let disconnectSpy: ReturnType<typeof vi.fn>

  beforeEach(() => {
    observeSpy = vi.fn()
    disconnectSpy = vi.fn()
    class MockIntersectionObserver {
      constructor(callback: IntersectionObserverCallback) {
        observedCallback = callback
      }
      observe = observeSpy
      disconnect = disconnectSpy
      unobserve = vi.fn()
    }
    // @ts-expect-error minimal stub — only the members useInView touches
    global.IntersectionObserver = MockIntersectionObserver
  })

  afterEach(() => {
    // @ts-expect-error test cleanup
    delete global.IntersectionObserver
    observedCallback = null
  })

  it('starts out of view and observes the element', () => {
    const { getByTestId } = render(<Probe />)
    expect(getByTestId('probe').textContent).toBe('false')
    expect(observeSpy).toHaveBeenCalledTimes(1)
  })

  it('flips to true once an intersecting entry fires', () => {
    const { getByTestId } = render(<Probe />)
    act(() => {
      observedCallback?.(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })
    expect(getByTestId('probe').textContent).toBe('true')
  })

  it('ignores non-intersecting entries', () => {
    const { getByTestId } = render(<Probe />)
    act(() => {
      observedCallback?.(
        [{ isIntersecting: false } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })
    expect(getByTestId('probe').textContent).toBe('false')
  })

  it('is sticky — stays true after becoming visible even if observer would fire again', () => {
    const { getByTestId } = render(<Probe />)
    act(() => {
      observedCallback?.(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })
    expect(getByTestId('probe').textContent).toBe('true')
    // Once in view, the observer is disconnected — no further callbacks matter.
    expect(disconnectSpy).toHaveBeenCalled()
  })

  it('falls back to true immediately when IntersectionObserver is unavailable', () => {
    // @ts-expect-error test cleanup
    delete global.IntersectionObserver
    const { getByTestId } = render(<Probe />)
    expect(getByTestId('probe').textContent).toBe('true')
  })
})
