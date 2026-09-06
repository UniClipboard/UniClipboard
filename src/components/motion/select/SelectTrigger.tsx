import { Select as SelectPrimitive } from '@base-ui/react/select'
import { m, useReducedMotion, type HTMLMotionProps } from 'framer-motion'
import { ChevronDownIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

// Adapted from beui.dev/components/motion/select. Base UI owns keyboard and focus behavior.
export function SelectTrigger({
  className,
  size = 'default',
  children,
  ...props
}: SelectPrimitive.Trigger.Props & { size?: 'sm' | 'default' }) {
  const reduce = useReducedMotion()
  return (
    <SelectPrimitive.Trigger
      data-slot="select-trigger"
      data-cuelume-toggle="press"
      data-size={size}
      className={cn(
        'relative flex w-fit max-w-full items-center justify-between gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm text-card-foreground outline-none transition-colors hover:border-foreground/25 focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive data-placeholder:text-muted-foreground data-[size=default]:h-9 data-[size=sm]:h-7 [&_svg]:shrink-0',
        className
      )}
      {...props}
    >
      {children}
      <SelectPrimitive.Icon
        render={(iconProps, state) => (
          <m.span
            {...(iconProps as HTMLMotionProps<'span'>)}
            aria-hidden="true"
            initial={false}
            animate={{ rotate: state.open ? 180 : 0 }}
            transition={reduce ? { duration: 0 } : { type: 'spring', duration: 0.4, bounce: 0.3 }}
            className="shrink-0 text-muted-foreground"
          >
            <ChevronDownIcon className="size-4" />
          </m.span>
        )}
      />
    </SelectPrimitive.Trigger>
  )
}
