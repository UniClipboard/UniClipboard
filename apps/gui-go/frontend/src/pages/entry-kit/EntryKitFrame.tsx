import type { ReactNode } from 'react'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { UiRoot } from '@/ui-kit/first-frame'

/**
 * Hosts one entry page (setup, startup, recovery, unlock) of the platform UI kit below the window's
 * title bar: the kit root takes the remaining height and follows the applied app theme.
 */
export function EntryKitFrame({ children }: { children: ReactNode }) {
  const colorScheme = useAppliedColorScheme()
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="entry-kit">
      <UiRoot colorScheme={colorScheme}>
        <div className="flex h-full min-h-0 flex-col">{children}</div>
      </UiRoot>
    </div>
  )
}
