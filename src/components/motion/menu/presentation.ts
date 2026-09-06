import type { Transition } from 'framer-motion'
import { EASE_OUT } from '@/lib/ease'

export const MENU_SURFACE_CLASS =
  'rounded-xl border border-border bg-card p-1.5 text-foreground outline-none'
export const MENU_SHADOW_CLASS = '[filter:drop-shadow(0_1.125rem_1.75rem_rgba(0,0,0,0.2))]'
export const MENU_ITEM_CLASS =
  'relative isolate flex w-full select-none items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[0.8125rem] outline-none'

export function menuMotion(
  open: boolean,
  reduce: boolean,
  collapsedClip: string,
  keyboard = false
) {
  const transition: Transition = keyboard
    ? { duration: 0 }
    : reduce
      ? { duration: 0.1, ease: EASE_OUT }
      : {
          clipPath: { duration: 0.3, ease: EASE_OUT },
          opacity: { duration: 0.3, ease: EASE_OUT },
        }
  return {
    animate: {
      opacity: open ? 1 : 0,
      clipPath: reduce || keyboard || open ? 'inset(0px 0px 0px 0px round 12px)' : collapsedClip,
    },
    transition,
  }
}

export function anchoredMenuClip(side: string | undefined) {
  switch (side) {
    case 'top':
      return 'inset(calc(100% - 16px) calc(50% - 8px) 0px calc(50% - 8px) round 10px)'
    case 'left':
      return 'inset(calc(50% - 8px) 0px calc(50% - 8px) calc(100% - 16px) round 10px)'
    case 'right':
      return 'inset(calc(50% - 8px) calc(100% - 16px) calc(50% - 8px) 0px round 10px)'
    default:
      return 'inset(0px calc(50% - 8px) calc(100% - 16px) calc(50% - 8px) round 10px)'
  }
}
