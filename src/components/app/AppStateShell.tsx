import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import appIcon from '@/updater/app-icon.png'

type AppStateShellProps = {
  category?: ReactNode
  categoryIcon?: ReactNode
  title: ReactNode
  description?: ReactNode
  children?: ReactNode
  categoryTone?: 'default' | 'destructive' | 'success'
}

const categoryToneClass = {
  default: 'text-muted-foreground',
  destructive: 'text-destructive',
  success: 'text-emerald-700 dark:text-emerald-400',
} as const

/**
 * Shared full-window layout for every state shown before the main window
 * content: startup, failure, profile recovery, unlock and first-run setup.
 */
export function AppStateShell({
  category,
  categoryIcon,
  title,
  description,
  children,
  categoryTone = 'default',
}: AppStateShellProps) {
  return (
    <main className="flex min-h-0 flex-1 overflow-y-auto bg-background text-foreground">
      <div className="m-auto flex w-full max-w-md flex-col px-6 pb-12 pt-6 sm:px-8">
        <div className="mb-10 flex items-center gap-2.5 text-ui-body font-semibold">
          <img src={appIcon} alt="" className="size-8 shrink-0" />
          <span>UniClipboard</span>
        </div>
        {category && (
          <div
            className={cn(
              'mb-3 flex items-center gap-2 text-ui-body',
              categoryToneClass[categoryTone]
            )}
          >
            {categoryIcon}
            {category}
          </div>
        )}
        <h1 className="text-ui-title font-semibold" aria-live="polite">
          {title}
        </h1>
        {description && (
          <p className="mt-2 text-ui-body-relaxed text-muted-foreground">{description}</p>
        )}
        {children}
      </div>
    </main>
  )
}
