import React, { ReactNode } from 'react'

interface SettingContentLayoutProps {
  children: ReactNode
}

const SettingContentLayout: React.FC<SettingContentLayoutProps> = ({ children }) => {
  return <div className="mx-auto flex w-full max-w-3xl flex-col gap-8">{children}</div>
}

export default SettingContentLayout
