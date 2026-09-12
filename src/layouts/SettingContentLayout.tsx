import React, { ReactNode } from 'react'

interface SettingContentLayoutProps {
  children: ReactNode
  header?: ReactNode
}

const SettingContentLayout: React.FC<SettingContentLayoutProps> = ({ children, header }) => {
  return (
    <div className="mx-auto w-full max-w-3xl ">
      {header}
      <div className="flex flex-col gap-8">{children}</div>
    </div>
  )
}

export default SettingContentLayout
