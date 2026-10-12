import type { DetailHeaderProps } from '@/ui-kit/contract'
import { Button } from './Button'

export function DetailHeader({ children, pin, delete: deleteAction }: DetailHeaderProps) {
  return (
    <div className="flex shrink-0 items-center gap-2">
      {children}
      <Button
        variant="subtle"
        icon="pin"
        label={pin.label}
        tooltip={pin.tooltip}
        onClick={pin.onClick}
      />
      <Button
        variant="subtle"
        icon="delete"
        label={deleteAction.label}
        onClick={deleteAction.onClick}
      />
    </div>
  )
}
