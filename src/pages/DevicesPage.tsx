import React from 'react'
import { DeviceList } from '@/components'

const DevicesPage: React.FC = () => {
  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <div className="h-full overflow-y-auto scrollbar-thin px-4 pb-12 pt-4 scroll-smooth">
          <div className="mb-12">
            <DeviceList />
          </div>
        </div>
      </div>
    </div>
  )
}

export default DevicesPage
