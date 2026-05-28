import React from 'react'

const Rules: React.FC = () => {
  return (
    <>
      <div className="mb-6">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-medium text-gray-400">设备同步规则</h3>
          <div className="flex-grow ml-3 border-t border-gray-800/50"></div>
        </div>

        {/* iPhone规则设置卡片 */}
        <div className="bg-gray-900 rounded-lg border border-gray-800/50 p-4 mb-3">
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center">
              <div className="flex-shrink-0 size-8 bg-blue-500/20 rounded-lg flex items-center justify-center">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  className="size-5 text-blue-400"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    d="M12 18h.01M8 21h8a2 2 0 002-2V5a2 2 0 00-2-2H8a2 2 0 00-2 2v14a2 2 0 002 2z"
                  />
                </svg>
              </div>
              <h4 className="ml-3 font-medium text-white">iPhone 13 同步规则</h4>
            </div>
            <button
              type="button"
              className="text-xs px-3 py-1 border border-gray-700 rounded-md text-gray-400 hover:bg-gray-700"
            >
              恢复默认
            </button>
          </div>

          {/* 规则列表 */}
          <div className="space-y-3">
            {/* 规则项 */}
            <div className="flex items-center justify-between bg-gray-800/70 rounded-md p-3">
              <div>
                <h5 className="text-sm font-medium text-white">自动同步</h5>
                <p className="text-xs text-gray-400 mt-0.5">在设备解锁状态下自动同步剪贴板内容</p>
              </div>
              <label className="flex items-center">
                <div className="relative">
                  <input
                    type="checkbox"
                    aria-label="自动同步"
                    className="sr-only"
                    checked
                    readOnly
                  />
                  <div className="block bg-gray-600 w-10 h-5 rounded-full"></div>
                  <div className="dot absolute left-1 top-1 bg-white size-3 rounded-full transition transform translate-x-5"></div>
                </div>
              </label>
            </div>

            {/* 规则项 */}
            <div className="flex items-center justify-between bg-gray-800/70 rounded-md p-3">
              <div>
                <h5 className="text-sm font-medium text-white">同步文本</h5>
                <p className="text-xs text-gray-400 mt-0.5">允许同步文本内容</p>
              </div>
              <label className="flex items-center">
                <div className="relative">
                  <input
                    type="checkbox"
                    aria-label="同步文本"
                    className="sr-only"
                    checked
                    readOnly
                  />
                  <div className="block bg-gray-600 w-10 h-5 rounded-full"></div>
                  <div className="dot absolute left-1 top-1 bg-white size-3 rounded-full transition transform translate-x-5"></div>
                </div>
              </label>
            </div>

            {/* 规则项 */}
            <div className="flex items-center justify-between bg-gray-800/70 rounded-md p-3">
              <div>
                <h5 className="text-sm font-medium text-white">同步图片</h5>
                <p className="text-xs text-gray-400 mt-0.5">
                  允许同步图片内容 (可能会消耗更多流量)
                </p>
              </div>
              <label className="flex items-center">
                <div className="relative">
                  <input
                    type="checkbox"
                    aria-label="同步图片"
                    className="sr-only"
                    checked
                    readOnly
                  />
                  <div className="block bg-gray-600 w-10 h-5 rounded-full"></div>
                  <div className="dot absolute left-1 top-1 bg-white size-3 rounded-full transition transform translate-x-5"></div>
                </div>
              </label>
            </div>

            {/* 规则项 */}
            <div className="flex items-center justify-between bg-gray-800/70 rounded-md p-3">
              <div>
                <h5 className="text-sm font-medium text-white">同步文件</h5>
                <p className="text-xs text-gray-400 mt-0.5">允许同步文件内容 (最大10MB)</p>
              </div>
              <label className="flex items-center">
                <div className="relative">
                  <input type="checkbox" aria-label="同步文件" className="sr-only" />
                  <div className="block bg-gray-600 w-10 h-5 rounded-full"></div>
                  <div className="dot absolute left-1 top-1 bg-white size-3 rounded-full transition"></div>
                </div>
              </label>
            </div>

            {/* 高级设置 */}
            <div className="pt-2">
              <button
                type="button"
                className="text-violet-400 hover:text-violet-300 text-xs flex items-center"
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  className="size-4 mr-1"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    d="M19 9l-7 7-7-7"
                  />
                </svg>
                显示高级设置
              </button>
            </div>
          </div>
        </div>

        {/* 工作站规则折叠卡片 */}
        <div className="bg-gray-900 rounded-lg border border-gray-800/50 p-4 mb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center">
              <div className="flex-shrink-0 size-8 bg-purple-500/20 rounded-lg flex items-center justify-center">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  className="size-5 text-purple-400"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"
                  />
                </svg>
              </div>
              <h4 className="ml-3 font-medium text-white">工作站 同步规则</h4>
            </div>
            <button
              type="button"
              aria-label="展开 工作站 同步规则"
              className="text-gray-400 hover:text-white"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="size-5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M19 9l-7 7-7-7"
                />
              </svg>
            </button>
          </div>
        </div>

        {/* iPad规则折叠卡片 */}
        <div className="bg-gray-900 rounded-lg border border-gray-800/50 p-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center">
              <div className="flex-shrink-0 size-8 bg-green-500/20 rounded-lg flex items-center justify-center">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  className="size-5 text-green-400"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="2"
                    d="M12 18h.01M7 21h10a2 2 0 002-2V5a2 2 0 00-2-2H7a2 2 0 00-2 2v14a2 2 0 002 2z"
                  />
                </svg>
              </div>
              <h4 className="ml-3 font-medium text-white">iPad Pro 同步规则</h4>
            </div>
            <button
              type="button"
              aria-label="展开 iPad Pro 同步规则"
              className="text-gray-400 hover:text-white"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="size-5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M19 9l-7 7-7-7"
                />
              </svg>
            </button>
          </div>
        </div>
      </div>
    </>
  )
}

export default Rules
