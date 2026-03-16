import React, { useEffect } from 'react'
import { PairedDevicesPanel, ThisDeviceCard } from '@/components'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useAppDispatch } from '@/store/hooks'
import { fetchLocalDeviceInfo } from '@/store/slices/devicesSlice'

const DevicesPage: React.FC = () => {
  const dispatch = useAppDispatch()

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
  }, [dispatch])

  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <ScrollArea className="h-full">
          <div className="px-4 pt-6 pb-8 space-y-4">
            <ThisDeviceCard />
            <PairedDevicesPanel />
          </div>
        </ScrollArea>
      </div>
    </div>
  )
}

export default DevicesPage
