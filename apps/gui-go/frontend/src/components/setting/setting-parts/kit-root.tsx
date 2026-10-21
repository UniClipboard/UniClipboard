import type { ReactNode } from 'react'
import { useAppliedColorScheme } from '@/hooks/useAppliedColorScheme'
import { UiRoot } from '@/ui-kit'

/** Also scopes a settings-owned alert presented outside the settings page. */
export function SettingPresentationRoot({ children }: { children: ReactNode }) {
  const colorScheme = useAppliedColorScheme()
  return <UiRoot colorScheme={colorScheme}>{children}</UiRoot>
}
