import React from 'react'
import { PairedDevicesPanel } from '@/components'

const DevicesPage: React.FC = () => {
  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <div className="h-full overflow-y-auto scrollbar-thin scroll-smooth">
          <PairedDevicesPanel />
        </div>
      </div>
    </div>
  )
}

export default DevicesPage
