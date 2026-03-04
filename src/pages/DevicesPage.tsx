import React from 'react'
import { PairedDevicesPanel } from '@/components'

const DevicesPage: React.FC = () => {
  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <div className="h-full overflow-y-auto scrollbar-thin px-4 pt-6 pb-8 scroll-smooth">
          <div className="mb-8">
            <PairedDevicesPanel />
          </div>
        </div>
      </div>
    </div>
  )
}

export default DevicesPage
