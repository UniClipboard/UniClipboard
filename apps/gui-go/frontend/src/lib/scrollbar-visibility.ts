const SCROLLBAR_HIDE_DELAY_MS = 500

export function initializeScrollbarVisibility(doc: Document = document): () => void {
  const hideTimers = new Map<HTMLElement, ReturnType<typeof setTimeout>>()

  const handleScroll = (event: Event) => {
    const element = getScrollElement(event.target, doc)
    if (!element) return

    element.dataset.scrollActive = 'true'
    const previousTimer = hideTimers.get(element)
    if (previousTimer) clearTimeout(previousTimer)

    const timer = setTimeout(() => {
      delete element.dataset.scrollActive
      hideTimers.delete(element)
    }, SCROLLBAR_HIDE_DELAY_MS)
    hideTimers.set(element, timer)
  }

  doc.addEventListener('scroll', handleScroll, true)

  return () => {
    doc.removeEventListener('scroll', handleScroll, true)
    hideTimers.forEach((timer, element) => {
      clearTimeout(timer)
      delete element.dataset.scrollActive
    })
    hideTimers.clear()
  }
}

function getScrollElement(target: EventTarget | null, doc: Document): HTMLElement | null {
  if (target instanceof HTMLElement) return target
  return doc.scrollingElement instanceof HTMLElement ? doc.scrollingElement : null
}
