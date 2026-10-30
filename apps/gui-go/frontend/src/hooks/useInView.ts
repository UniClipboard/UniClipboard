import { useEffect, useRef, useState } from 'react'

/**
 * Tracks whether the returned ref's element intersects the viewport.
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
    const el = ref.current
    if (!el) return
    if (typeof IntersectionObserver === 'undefined') {
      setInView(true)
      return
    }
    const observer = new IntersectionObserver(
      entries => {
        setInView(entries.some(entry => entry.isIntersecting))
      },
      { rootMargin }
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [rootMargin])

  return [ref, inView]
}
