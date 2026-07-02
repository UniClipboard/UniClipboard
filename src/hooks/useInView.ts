import { useEffect, useRef, useState } from 'react'

/**
 * Tracks whether the returned ref's element has ever intersected the
 * viewport. Sticky: once true, stays true — consumers gating a one-time fetch
 * on this (e.g. deferred thumbnail loading in a non-virtualized list) don't
 * want to re-fetch every time the element scrolls in and out of view.
 *
 * Falls back to `true` immediately in environments without
 * `IntersectionObserver` (e.g. some test runners) so gated consumers degrade
 * to "always enabled" rather than never loading.
 *
 * @param rootMargin Passed to `IntersectionObserver` — expand to start
 *   resolving slightly before the element is actually on-screen.
 */
export function useInView<T extends Element>(
  rootMargin = '200px'
): [React.RefObject<T | null>, boolean] {
  const ref = useRef<T | null>(null)
  const [inView, setInView] = useState(false)

  useEffect(() => {
    if (inView) return
    const el = ref.current
    if (!el) return
    if (typeof IntersectionObserver === 'undefined') {
      setInView(true)
      return
    }
    const observer = new IntersectionObserver(
      entries => {
        if (entries.some(entry => entry.isIntersecting)) setInView(true)
      },
      { rootMargin }
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [inView, rootMargin])

  return [ref, inView]
}
