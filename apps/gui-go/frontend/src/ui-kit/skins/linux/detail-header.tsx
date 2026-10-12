import type { DetailHeaderProps } from '@/ui-kit/contract'

export function DetailHeader({ children }: DetailHeaderProps) {
  return <div className="flex shrink-0 items-center gap-2">{children}</div>
}
