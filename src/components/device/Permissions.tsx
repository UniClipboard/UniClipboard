import React from 'react'

const Permissions: React.FC = () => {
  return (
    <>
      {' '}
      <div className="mb-6">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-medium text-gray-400">权限管理</h3>
          <div className="flex-grow ml-3 border-t border-gray-800/50"></div>
        </div>

        <div className="bg-gray-800 rounded-lg p-4">
          <div className="mb-4">
            <h4 className="font-medium text-white mb-2">设备访问权限</h4>
            <p className="text-sm text-gray-400">
              控制每个设备可以访问和同步的内容类型。通过限制权限，您可以保护敏感信息不被所有设备访问。
            </p>
          </div>

          {/* 权限表格 */}
          <div className="overflow-x-auto">
            <table className="min-w-full text-sm">
              <thead>
                <tr className="border-b border-gray-800">
                  <th className="py-3 text-left text-gray-400 font-medium">设备</th>
                  <th className="py-3 text-center text-gray-400 font-medium">读取剪贴板</th>
                  <th className="py-3 text-center text-gray-400 font-medium">写入剪贴板</th>
                  <th className="py-3 text-center text-gray-400 font-medium">访问历史</th>
                  <th className="py-3 text-center text-gray-400 font-medium">文件传输</th>
                </tr>
              </thead>
              <tbody>
                <tr className="border-b border-gray-800">
                  <td className="py-3 flex items-center">
                    <div className="flex-shrink-0 size-6 bg-blue-500/20 rounded-md flex items-center justify-center mr-2">
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        className="size-3.5 text-blue-400"
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
                    <span className="text-white">iPhone 13</span>
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPhone 13 读取剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPhone 13 写入剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPhone 13 访问历史"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPhone 13 文件传输"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                    />
                  </td>
                </tr>
                <tr className="border-b border-gray-800">
                  <td className="py-3 flex items-center">
                    <div className="flex-shrink-0 size-6 bg-purple-500/20 rounded-md flex items-center justify-center mr-2">
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        className="size-3.5 text-purple-400"
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
                    <span className="text-white">工作站</span>
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="工作站 读取剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="工作站 写入剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="工作站 访问历史"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="工作站 文件传输"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                </tr>
                <tr>
                  <td className="py-3 flex items-center">
                    <div className="flex-shrink-0 size-6 bg-green-500/20 rounded-md flex items-center justify-center mr-2">
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        className="size-3.5 text-green-400"
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
                    <span className="text-white">iPad Pro</span>
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPad Pro 读取剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPad Pro 写入剪贴板"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                      checked
                      readOnly
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPad Pro 访问历史"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                    />
                  </td>
                  <td className="py-3 text-center">
                    <input
                      type="checkbox"
                      aria-label="iPad Pro 文件传输"
                      className="form-checkbox size-4 text-violet-500 rounded focus:ring-0 focus:ring-offset-0 border-gray-600 bg-gray-700"
                    />
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <div className="mt-4 text-right">
            <button
              type="button"
              className="px-3 py-1.5 bg-violet-500 hover:bg-violet-400 text-white text-sm rounded-md transition duration-150"
            >
              保存权限设置
            </button>
          </div>
        </div>
      </div>
    </>
  )
}

export default Permissions
