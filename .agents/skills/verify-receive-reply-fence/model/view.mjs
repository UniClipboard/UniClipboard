export const title = '接收进度失效响应不能发布'
export const state = v => v.staleCommitted ? '旧响应已发布' :
  v.cancelled ? (v.phase === 'idle' ? '详情已关闭' : '关闭后等待响应') :
  v.phase === 'idle' ? '等待刷新' :
  v.ticket !== v.generation ? '请求已失效' : '等待读取响应'
export const step = action => ({ StartRead: '开始读取', ReceiveReply: '响应返回',
  DetailReply: '继续读取', ConfirmReply: '响应返回', Retire: '使请求失效',
  Unmount: '关闭详情', Done: '保持结束状态' })[action] ?? action
