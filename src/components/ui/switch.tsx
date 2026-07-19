import { MotionConfig, m, useReducedMotion } from 'framer-motion'
import * as React from 'react'
import { cn } from '@/lib/utils'

// Motion feel ported from beui.dev/components/motion/switch: a heavy, deliberate
// thumb (high mass, no wobble) that springs across on toggle and squishes on
// press. The public API stays identical to the existing shared switch
// (controlled checked/onCheckedChange + size + arbitrary button props like
// id/aria-label), so no call site changes.
const THUMB_SPRING = { type: 'spring', stiffness: 800, damping: 80, mass: 4 } as const

const SIZES = {
  default: { track: 'h-[18.4px] w-[32px] px-[2px]', thumb: 'size-4' },
  sm: { track: 'h-[14px] w-[24px] px-[1px]', thumb: 'size-3' },
} as const

// Drag/animation handlers collide with framer-motion's same-named props, so drop
// them from the public surface (no call site uses them on a switch).
type MotionConflictingProps =
  | 'onDrag'
  | 'onDragStart'
  | 'onDragEnd'
  | 'onDragEnter'
  | 'onDragExit'
  | 'onDragLeave'
  | 'onDragOver'
  | 'onAnimationStart'
  | 'onAnimationEnd'
  | 'onAnimationIteration'

export interface SwitchProps extends Omit<
  React.ComponentProps<'button'>,
  'onChange' | 'onClick' | 'children' | MotionConflictingProps
> {
  checked: boolean
  onCheckedChange?: (checked: boolean) => void
  size?: 'sm' | 'default'
}

function Switch({
  checked,
  onCheckedChange,
  disabled,
  size = 'default',
  className,
  ...props
}: SwitchProps) {
  const reduce = useReducedMotion()
  const [isPressed, setIsPressed] = React.useState(false)
  const [isPointer, setIsPointer] = React.useState(false)

  const squish = !disabled && isPointer && isPressed && !reduce
  const dims = SIZES[size]

  return (
    <MotionConfig transition={reduce ? { duration: 0 } : THUMB_SPRING}>
      <m.button
        {...props}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        data-slot="switch"
        data-size={size}
        data-state={checked ? 'checked' : 'unchecked'}
        // cuelume plays a toggle sound on click of [data-cuelume-toggle] (see lib/ui-sound.ts).
        data-cuelume-toggle=""
        onClick={() => !disabled && onCheckedChange?.(!checked)}
        onPointerDown={e => {
          setIsPressed(true)
          setIsPointer(e.type.startsWith('pointer'))
        }}
        onPointerUp={() => setIsPressed(false)}
        onPointerLeave={() => setIsPressed(false)}
        onPointerCancel={() => setIsPressed(false)}
        initial={false}
        className={cn(
          'peer inline-flex shrink-0 cursor-pointer items-center rounded-full outline-none transition-colors duration-200',
          'focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
          'disabled:cursor-not-allowed disabled:opacity-50',
          dims.track,
          checked ? 'justify-end bg-primary' : 'justify-start bg-input dark:bg-input/80',
          className
        )}
      >
        <m.div
          layout
          animate={{ scale: squish ? 0.9 : 1 }}
          className={cn(
            'pointer-events-none block rounded-full bg-background shadow-sm dark:bg-foreground',
            dims.thumb
          )}
        />
      </m.button>
    </MotionConfig>
  )
}

export { Switch }
