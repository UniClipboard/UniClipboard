import { Select as SelectPrimitive } from '@base-ui/react/select'
import { CheckIcon } from 'lucide-react'
import { MenuHighlight } from '@/components/motion/menu/MenuHighlight'
import { MENU_ITEM_CLASS } from '@/components/motion/menu/presentation'
import { cn } from '@/lib/utils'

export function SelectItem({ className, children, ...props }: SelectPrimitive.Item.Props) {
  return (
    <SelectPrimitive.Item
      data-slot="select-item"
      data-cuelume-toggle="release"
      className={cn(
        MENU_ITEM_CLASS,
        'cursor-default pr-8 text-foreground data-disabled:pointer-events-none data-disabled:opacity-40 [&_svg]:shrink-0',

        className
      )}
      render={(renderProps, state) => (
        <div {...renderProps}>
          {state.highlighted && <MenuHighlight layoutId="select-active" />}
          {renderProps.children}
        </div>
      )}
      {...props}
    >
      <SelectPrimitive.ItemText className="flex min-w-0 flex-1 items-center gap-2 break-words">
        {children}
      </SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator className="pointer-events-none absolute right-2.5 flex size-3.5 items-center justify-center">
        <CheckIcon className="size-3.5" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  )
}
