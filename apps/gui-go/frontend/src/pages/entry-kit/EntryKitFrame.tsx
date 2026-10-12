import type { ReactNode } from 'react'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { UiRoot } from '@/ui-kit/first-frame'

/**
 * Hosts one entry page (setup, startup, recovery, unlock) of the platform UI kit below the window's
 * title bar: the kit root takes the remaining height and follows the applied app theme. The title bar
 * is transparent, so the kit surface is pulled up behind it (-mt-10 on the root, pt-10 inside) and
 * the strip with the window buttons shows the same background as the page.
 */
export function EntryKitFrame({ children }: { children: ReactNode }) {
  const colorScheme = useAppliedColorScheme()
  return (
    <div className="-mt-10 flex min-h-0 flex-1 flex-col" data-testid="entry-kit">
      <UiRoot colorScheme={colorScheme}>
        <div className="flex h-full min-h-0 flex-col pt-10">{children}</div>
      </UiRoot>
    </div>
  )
}
