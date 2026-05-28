import React from 'react'

// eslint-disable-next-line @typescript-eslint/no-empty-object-type
interface WelcomeProps {}

const Welcome: React.FC<WelcomeProps> = () => {
  return (
    <>
      <div className="bg-gray-800 mx-4 mt-4 rounded-xl overflow-hidden border border-gray-700/30">
        <div className="p-5 flex flex-col md:flex-row items-center justify-between">
          <div className="mb-4 md:mb-0 md:mr-6">
            <h2 className="text-lg font-semibold text-white mb-1">连接新设备到 ClipSync</h2>
            <p className="text-gray-400 text-sm max-w-xl">
              通过配对码或扫描二维码将新设备连接到您的 ClipSync
              网络。所有已配对设备将自动同步剪贴板内容。
            </p>
          </div>
          <div className="flex gap-x-3">
            {/* <button className="bg-gray-700 hover:bg-gray-600 text-white px-4 py-2 rounded-lg text-sm font-medium transition duration-150 flex items-center">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="size-5 mr-1.5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M12 4v1m6 11h2m-6 0h-2v4m0-11v3m0 0h.01M12 12h4.01M16 20h4M4 12h4m12 0h.01M5 8h2a1 1 0 001-1V5a1 1 0 00-1-1H5a1 1 0 00-1 1v2a1 1 0 001 1zm12 0h2a1 1 0 001-1V5a1 1 0 00-1-1h-2a1 1 0 00-1 1v2a1 1 0 001 1zM5 20h2a1 1 0 001-1v-2a1 1 0 00-1-1H5a1 1 0 00-1 1v2a1 1 0 001 1z"
                />
              </svg>
              扫描二维码
            </button> */}
            <button
              type="button"
              className="bg-violet-500 hover:bg-violet-400 text-white px-4 py-2 rounded-lg text-sm font-medium transition duration-150 flex items-center"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                className="size-5 mr-1.5"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z"
                />
              </svg>
              生成配对码
            </button>
          </div>
        </div>
      </div>
    </>
  )
}

export default Welcome
