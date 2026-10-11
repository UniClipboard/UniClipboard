import { ToolbarGroup as FluentToolbarGroup } from '@fluentui/react-components'
import type { ToolbarGroupProps } from '@/ui-kit/contract'

/** Items that share one surface. A WinUI CommandBar draws no capsule around them: a plain Fluent toolbar group. */
export function ToolbarGroup({ children }: ToolbarGroupProps) {
  return <FluentToolbarGroup>{children}</FluentToolbarGroup>
}
