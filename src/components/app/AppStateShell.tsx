import type { ReactNode } from 'react'
import appIcon from '@/updater/app-icon.png'

type AppStateShellProps = {
  category?: ReactNode
  categoryIcon?: ReactNode
  title: ReactNode
  description: ReactNode
  children?: ReactNode
  categoryTone?: 'default' | 'destructive' | 'success'
  width?: 'default' | 'compact'
}

const categoryToneClass = {
  default: 'text-muted-foreground',
  destructive: 'text-destructive',
  success: 'text-emerald-600 dark:text-emerald-400',
} as const

export function AppStateShell({
  category,
  categoryIcon,
  title,
  description,
  children,
  categoryTone = 'default',
  width = 'default',
}: AppStateShellProps) {
  return (
    <main className="flex min-h-0 flex-1 overflow-y-auto bg-background text-foreground">
      <div
        className={`m-auto w-full px-6 py-12 sm:px-12 ${width === 'compact' ? 'max-w-lg' : 'max-w-2xl'}`}
      >
        <div className="mb-10 flex items-center gap-3">
          <img src={appIcon} alt="" className="size-10 shrink-0" />
          <span className="text-ui-section font-semibold">UniClipboard</span>
        </div>
        {category && (
          <div
            className={`mb-4 flex items-center gap-2 text-ui-body ${categoryToneClass[categoryTone]}`}
          >
            {categoryIcon}
            {category}
          </div>
        )}
        <h1 className="text-ui-title font-semibold" aria-live="polite">
          {title}
        </h1>
        <p className="mt-3 text-ui-body text-muted-foreground">{description}</p>
        {children}
      </div>
    </main>
  )
}
