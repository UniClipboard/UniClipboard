import React from 'react'

const DeviceSectionLabel: React.FC<{
  label: string
  children?: React.ReactNode
}> = ({ label, children }) => (
  <div className="flex items-center justify-between px-2.5 pb-1 pt-4">
    <div className="flex min-w-0 items-center gap-1.5">
      <span className="text-ui-caption font-semibold uppercase text-muted-foreground/80">
        {label}
      </span>
    </div>
    {children}
  </div>
)

export default DeviceSectionLabel
