import React from 'react'

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
interface DeviceStatusProps {}

const DeviceStatus: React.FC<DeviceStatusProps> = () => {
  return (
    <>
      <div className="bg-gray-800 border-b border-gray-800/50 px-4 py-2 flex items-center space-x-3 overflow-x-auto hide-scrollbar">
        <div className="flex items-center text-sm text-gray-400">
          <span>已连接设备:</span>
        </div>
        <div className="px-3 py-1 bg-gray-700/70 rounded-full flex items-center">
          <div className="h-2 w-2 rounded-full bg-green-500 mr-2"></div>
          <span className="text-xs text-white">MacBook Pro</span>
        </div>
        <div className="px-3 py-1 bg-gray-700/70 rounded-full flex items-center">
          <div className="h-2 w-2 rounded-full bg-green-500 mr-2"></div>
          <span className="text-xs text-white">iPhone 13</span>
        </div>
        <div className="px-3 py-1 bg-gray-700/70 rounded-full flex items-center">
          <div className="h-2 w-2 rounded-full bg-yellow-500 mr-2"></div>
          <span className="text-xs text-white">工作站</span>
        </div>
        <div className="px-3 py-1 border border-dashed border-gray-600 rounded-full flex items-center hover:bg-gray-700/60">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            className="h-3 w-3 text-gray-400 mr-1"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 6v6m0 0v6m0-6h6m-6 0H6"
            ></path>
          </svg>
          <span className="text-xs text-gray-400">添加设备</span>
        </div>
      </div>
    </>
  )
}

export default DeviceStatus
