import { Select as SelectPrimitive } from '@base-ui/react/select'
import { LayoutGroup, m, useReducedMotion, type HTMLMotionProps } from 'framer-motion'
import { useId } from 'react'
import {
  anchoredMenuClip,
  MENU_SURFACE_CLASS,
  MENU_SHADOW_CLASS,
  menuMotion,
} from '@/components/motion/menu/presentation'
import { cn } from '@/lib/utils'

// Keep select semantics while sharing the action menus' surface and reveal motion.
export function SelectContent({
  className,
  children,
  side = 'bottom',
  sideOffset = 8,
  align = 'start',
  alignOffset = 0,
  alignItemWithTrigger = false,
  ...props
}: SelectPrimitive.Popup.Props &
  Pick<
    SelectPrimitive.Positioner.Props,
    'align' | 'alignOffset' | 'side' | 'sideOffset' | 'alignItemWithTrigger'
  >) {
  const reduce = useReducedMotion() ?? false
  const layoutId = useId()
  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Positioner
        side={side}
        sideOffset={sideOffset}
        align={align}
        alignOffset={alignOffset}
        alignItemWithTrigger={alignItemWithTrigger}
        className={cn('isolate z-50', MENU_SHADOW_CLASS)}
      >
        <SelectPrimitive.Popup
          data-slot="select-content"
          className={cn(
            'relative max-h-(--available-height) w-(--anchor-width) min-w-36 origin-(--transform-origin) overflow-x-hidden overflow-y-auto',
            anchoredMenuClip,
            MENU_SURFACE_CLASS,
            className
          )}
          render={(renderProps, state) => {
            const collapsed = anchoredMenuClip(state.side)
            return (
              <m.div
                {...(renderProps as HTMLMotionProps<'div'>)}
                initial={menuMotion(false, reduce, collapsed).animate}
                {...menuMotion(state.open, reduce, collapsed)}
              />
            )
          }}
          {...props}
        >
          <LayoutGroup id={layoutId}>
            <SelectPrimitive.List>{children}</SelectPrimitive.List>
          </LayoutGroup>
        </SelectPrimitive.Popup>
      </SelectPrimitive.Positioner>
    </SelectPrimitive.Portal>
  )
}
