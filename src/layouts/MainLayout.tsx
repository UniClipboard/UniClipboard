import React, { ReactNode } from 'react'
import { Sidebar } from '@/components'
import InsetSurface from '@/components/layout/InsetSurface'
import { usePlatform } from '@/hooks/usePlatform'

interface MainLayoutProps {
  children: ReactNode
}

/**
 * Linux 系统标题栏布局。
 *
 * Linux/Tauri 当前使用系统窗口标题栏，主内容区不再模拟 macOS/Windows 的内嵌圆角面板。
 */
const LinuxMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  return (
    <>
      <Sidebar className="border-r border-border/40 bg-background/80 dark:bg-background/60" />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-card text-card-foreground">
        {children}
      </main>
    </>
  )
}

/**
 * 自定义标题栏布局。
 *
 * macOS/Windows 继续使用内嵌内容面板，和自绘窗口标题栏保持一致。
 */
const InsetMainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  return (
    <>
      <Sidebar />

      <main className="relative flex min-h-0 flex-1 flex-col overflow-hidden pb-2 pr-2">
        <InsetSurface className="flex-1 w-full h-full">{children}</InsetSurface>
      </main>
    </>
  )
}

const MainLayout: React.FC<MainLayoutProps> = ({ children }) => {
  const { isLinux, isTauri } = usePlatform()

  if (isLinux && isTauri) {
    return <LinuxMainLayout>{children}</LinuxMainLayout>
  }

  return <InsetMainLayout>{children}</InsetMainLayout>
}

export default MainLayout
