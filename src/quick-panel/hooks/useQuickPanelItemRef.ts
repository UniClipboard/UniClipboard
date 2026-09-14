import { useCallback } from 'react'

/** Register mounted rows for keyboard scrolling, with a stable callback per index. */
export function useQuickPanelItemRef(itemRefs: Map<number, HTMLDivElement>, index: number) {
  return useCallback(
    (element: HTMLDivElement | null) => {
      if (element) itemRefs.set(index, element)
      else itemRefs.delete(index)
    },
    [itemRefs, index]
  )
}
