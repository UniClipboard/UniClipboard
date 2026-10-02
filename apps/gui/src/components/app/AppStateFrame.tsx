import type { ReactNode } from 'react'

type AppStateFrameProps = {
  titleBar: ReactNode
  children: ReactNode
}

export function AppStateFrame({ titleBar, children }: AppStateFrameProps) {
  return (
    <div className="flex h-full w-full flex-col bg-background">
      {titleBar}
      {children}
    </div>
  )
}
