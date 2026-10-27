import { useEffect, useEffectEvent, useRef, type KeyboardEvent } from 'react'
import type { SplitViewProps } from '@/ui-kit/contract'

/** Own focus and dismissal for the two adaptive navigation layers. */
export function useSplitNavigation({
  sidebarHidden,
  sidebarOverlay,
  sidebarToggle,
  detailNavigation,
  singlePane = false,
}: Pick<
  SplitViewProps,
  'sidebarHidden' | 'sidebarOverlay' | 'sidebarToggle' | 'detailNavigation'
> & { singlePane?: boolean }) {
  const root = useRef<HTMLDivElement>(null)
  const sidebarRef = useRef<HTMLDivElement>(null)
  const toggleRef = useRef<HTMLButtonElement>(null)
  const backRef = useRef<HTMLButtonElement>(null)
  const single = singlePane && Boolean(detailNavigation)
  const detail = single && detailNavigation?.open === true
  const overlay = Boolean(sidebarOverlay && sidebarToggle)
  const drawerOpen = overlay && !sidebarHidden
  const previousDrawer = useRef(false)
  const previousDetail = useRef(false)
  const dismissSidebar = useEffectEvent(() => sidebarToggle?.onClick())
  const closeDetail = useEffectEvent(() => detailNavigation?.onOpenChange(false))
  useEffect(() => {
    if (drawerOpen) {
      const selected = sidebarRef.current?.querySelector<HTMLElement>('[aria-current="page"]')
      const first = sidebarRef.current?.querySelector<HTMLElement>('button, [tabindex="0"]')
      ;(selected ?? first)?.focus({ preventScroll: true })
    } else if (previousDrawer.current) toggleRef.current?.focus({ preventScroll: true })
    previousDrawer.current = drawerOpen
  }, [drawerOpen])
  useEffect(() => {
    const active = document.activeElement
    const detailPane = root.current?.querySelector('[data-pane="detail"]')
    if (detail) {
      // An editor opened with the detail may already have taken focus.
      if (!detailPane?.contains(active)) backRef.current?.focus({ preventScroll: true })
    } else if (previousDetail.current) {
      // Search keeps its own focus when it returns to the results list.
      if (active === document.body || active === backRef.current || detailPane?.contains(active)) {
        root.current
          ?.querySelector<HTMLElement>(
            '[data-pane]:not([data-pane="detail"]) [role="listbox"], [data-pane]:not([data-pane="detail"]) [role="grid"]'
          )
          ?.focus({ preventScroll: true })
      }
    }
    previousDetail.current = detail
  }, [detail])
  // Modal layers consume Escape before the page's global shortcuts. Portalled menus and
  // dialogs keep their own Escape behavior, so only unhandled presses navigate back.
  useEffect(() => {
    if (!drawerOpen && !detail) return
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) return
      const target = event.target as Element | null
      if (
        target?.closest('[role="dialog"], [role="menu"], [role="listbox"]') &&
        !sidebarRef.current?.contains(target) &&
        !root.current?.querySelector('[data-pane="list"]')?.contains(target)
      )
        return
      event.preventDefault()
      event.stopImmediatePropagation()
      if (drawerOpen) dismissSidebar()
      else closeDetail()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [drawerOpen, detail])
  const onSidebarKeyDown = (event: KeyboardEvent) => {
    if (!drawerOpen || event.key !== 'Tab') return
    const controls = Array.from(
      sidebarRef.current?.querySelectorAll<HTMLElement>(
        'button:not([disabled]), a[href], input, [tabindex="0"]'
      ) ?? []
    ).filter(element => element.tabIndex >= 0 && element.getClientRects().length > 0)
    const first = controls[0]
    const last = controls[controls.length - 1]
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last?.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first?.focus()
    }
  }
  return {
    root,
    sidebarRef,
    toggleRef,
    backRef,
    single,
    detail,
    overlay,
    drawerOpen,
    onSidebarKeyDown,
    onSearchFocus: () => {
      if (detail) detailNavigation?.onOpenChange(false)
    },
  }
}
